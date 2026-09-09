use std::{env, error::Error, net::SocketAddr, path::PathBuf, time::Duration};

use pulse_service::{Administration, AppState, ServiceConfig, router};
use tokio::net::TcpListener;
use tokio::time::MissedTickBehavior;

const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:8080";
const DEFAULT_MAX_DATABASE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt().compact().init();
    let database_path =
        PathBuf::from(env::var("PULSE_DATABASE_PATH").unwrap_or_else(|_| "pulse.db".to_owned()));
    let max_database_bytes = parse_env("PULSE_MAX_DATABASE_BYTES", DEFAULT_MAX_DATABASE_BYTES)?;
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.as_slice() == ["--version"] {
        println!("pulse-service {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"] {
        println!("{}", usage());
        return Ok(());
    }
    if arguments
        .first()
        .is_some_and(|argument| argument != "serve")
    {
        return run_admin(&arguments, &database_path, max_database_bytes);
    }

    let listen_address: SocketAddr = env::var("PULSE_LISTEN")
        .unwrap_or_else(|_| DEFAULT_LISTEN_ADDRESS.to_owned())
        .parse()?;
    let allow_public = env::var("PULSE_ALLOW_PUBLIC_LISTEN").as_deref() == Ok("true");
    if !listen_address.ip().is_loopback() && !allow_public {
        return Err(
            "non-loopback PULSE_LISTEN requires PULSE_ALLOW_PUBLIC_LISTEN=true; put TLS and access control in front of Pulse"
                .into(),
        );
    }

    let mut config = ServiceConfig::with_database(database_path);
    config.retention_days = parse_env("PULSE_RETENTION_DAYS", config.retention_days)?;
    config.offline_after_seconds =
        parse_env("PULSE_OFFLINE_AFTER_SECONDS", config.offline_after_seconds)?;
    config.max_nodes = parse_env("PULSE_MAX_NODES", config.max_nodes)?;
    config.max_database_bytes = max_database_bytes;
    let state = AppState::open(&config)?;
    let listener = TcpListener::bind(listen_address).await?;
    let maintenance = tokio::spawn(retention_maintenance(state.clone()));

    tracing::info!(address = %listen_address, "Pulse Service listening");
    let serve_result = axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    maintenance.abort();
    let _ = maintenance.await;
    serve_result?;
    tracing::info!("Pulse Service stopped cleanly");
    Ok(())
}

async fn retention_maintenance(state: AppState) {
    let mut ticker = tokio::time::interval(Duration::from_hours(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        match state.maintain_retention().await {
            Ok(removed) if removed > 0 => {
                tracing::info!(removed, "expired monitoring snapshots pruned");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "retention maintenance failed");
            }
        }
    }
}

fn run_admin(
    arguments: &[String],
    database_path: &std::path::Path,
    max_database_bytes: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let administration = Administration::open(database_path, max_database_bytes)?;
    match arguments {
        [area, action] if area == "enrollment" && action == "create" => {
            let secret = administration.create_enrollment(600)?;
            println!("{}", serde_json::to_string_pretty(&secret)?);
        }
        [area, action, flag, ttl]
            if area == "enrollment" && action == "create" && flag == "--ttl-seconds" =>
        {
            let secret = administration.create_enrollment(ttl.parse()?)?;
            println!("{}", serde_json::to_string_pretty(&secret)?);
        }
        [area, action, id] if area == "enrollment" && action == "revoke" => {
            administration.revoke_enrollment(id)?;
            println!("enrollment token revoked: {id}");
        }
        [area, action, id] if area == "node" && action == "rotate" => {
            let token = administration.rotate_node_token(id)?;
            println!("{token}");
        }
        [area, action, id] if area == "node" && action == "revoke" => {
            administration.revoke_node(id)?;
            println!("node revoked: {id}");
        }
        [area, action, id] if area == "node" && action == "delete" => {
            administration.delete_node(id)?;
            println!("node and its snapshots deleted: {id}");
        }
        [command] if command == "audit" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&administration.audit_events(100)?)?
            );
        }
        [command, limit] if command == "audit" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&administration.audit_events(limit.parse()?)?)?
            );
        }
        [command] if command == "backup" => {
            println!("{}", administration.backup()?.display());
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: pulse-service [--version | serve | enrollment create [--ttl-seconds N] | enrollment revoke ID | node rotate ID | node revoke ID | node delete ID | audit [LIMIT] | backup]"
}

async fn shutdown_signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
}

fn parse_env<T>(name: &str, default: T) -> Result<T, Box<dyn Error + Send + Sync>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}
