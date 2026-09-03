use std::{env, error::Error};

use axum::{Json, Router, routing::get};
use pulse_protocol::HealthResponse;
use tokio::net::TcpListener;

const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1:8080";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().compact().init();

    let listen_address =
        env::var("PULSE_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN_ADDRESS.to_owned());
    let listener = TcpListener::bind(&listen_address).await?;

    tracing::info!(address = %listen_address, "Pulse Service listening");
    axum::serve(
        listener,
        Router::new().route("/healthz", get(health_response)),
    )
    .await?;

    Ok(())
}

async fn health_response() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}
