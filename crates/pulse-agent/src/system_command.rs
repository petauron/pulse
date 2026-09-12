//! Only two fixed, locally selected programs. No shell, PATH search, arbitrary
//! executable, arguments, or Agent credentials ever reach a child process.

use std::{net::IpAddr, path::Path, process::Stdio, time::Duration};

use tokio::{io::AsyncReadExt, process::Command};

const MAX_OUTPUT_BYTES: usize = 16 * 1024;

pub enum SystemCommand {
    Ping(IpAddr),
    NvidiaMetrics,
}

pub async fn run(command: SystemCommand, limit: Duration) -> Result<Vec<u8>, &'static str> {
    let mut process = match command {
        SystemCommand::Ping(address) => ping_command(address)?,
        SystemCommand::NvidiaMetrics => {
            let mut command = Command::new("/usr/bin/nvidia-smi");
            command.args([
                "--query-gpu=name,utilization.gpu,memory.total,memory.used,temperature.gpu",
                "--format=csv,noheader,nounits",
            ]);
            command
        }
    };
    process
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = process.spawn().map_err(|_| "unavailable")?;
    let stdout = child.stdout.take().ok_or("unavailable")?;
    let mut body = Vec::new();
    let outcome = tokio::time::timeout(limit, async {
        stdout
            .take((MAX_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut body)
            .await
            .map_err(|_| "probe_failed")?;
        if body.len() > MAX_OUTPUT_BYTES {
            return Err("response_too_large");
        }
        let status = child.wait().await.map_err(|_| "probe_failed")?;
        if !status.success() {
            return Err("probe_failed");
        }
        Ok(())
    })
    .await;
    match outcome {
        Ok(Ok(())) => Ok(body),
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err("timeout")
        }
    }
}

fn ping_command(address: IpAddr) -> Result<Command, &'static str> {
    #[cfg(target_os = "linux")]
    let paths = ["/usr/bin/ping", "/bin/ping"];
    #[cfg(target_os = "macos")]
    let paths = if address.is_ipv6() {
        ["/sbin/ping6", "/usr/sbin/ping6"]
    } else {
        ["/sbin/ping", "/usr/sbin/ping"]
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let paths: [&str; 0] = [];

    let path = paths
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .ok_or("unavailable")?;
    let mut command = Command::new(path);
    #[cfg(target_os = "linux")]
    command.arg(if address.is_ipv6() { "-6" } else { "-4" });
    // Address is an IpAddr, never raw Service-provided text or an option.
    command
        .args(["-n", "-c", "1", "-s", "16"])
        .arg(address.to_string());
    Ok(command)
}
