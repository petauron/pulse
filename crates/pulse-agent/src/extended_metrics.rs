//! Capability-based optional collectors. One background worker caches one
//! sample; Linux counters use bounded streaming reads, never a process table.

#[cfg(any(target_os = "linux", test))]
use std::io::Read;
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use std::{fs, path::Path};

use pulse_protocol::{GpuSnapshot, MAX_GPU_DEVICES};
use tokio::{sync::watch, task::JoinHandle};

use crate::system_command::{self, SystemCommand};

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(15);
#[cfg(any(target_os = "linux", test))]
const MAX_PROC_BYTES: usize = 8 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_PROCESS_ENTRIES: usize = 131_072;
#[cfg(target_os = "linux")]
const MAX_SMALL_FILE_BYTES: u64 = 4096;
#[cfg(any(target_os = "linux", test))]
const COUNTER_BUDGET: Duration = Duration::from_millis(250);

#[derive(Clone, Default)]
pub struct ExtendedSnapshot {
    pub process_count: Option<u64>,
    pub tcp_connection_count: Option<u64>,
    pub udp_connection_count: Option<u64>,
    pub gpus: Option<Vec<GpuSnapshot>>,
}

pub struct ExtendedCollector {
    latest: watch::Receiver<Option<(Instant, ExtendedSnapshot)>>,
    worker: Option<JoinHandle<()>>,
}

impl ExtendedCollector {
    pub fn start(enabled: bool, gpu_enabled: bool) -> Self {
        let (sender, latest) = watch::channel(None);
        let worker = enabled.then(|| {
            tokio::spawn(async move {
                loop {
                    // Only one blocking job exists at a time. No timeout/retry can
                    // accumulate blocked filesystem operations in Tokio's pool.
                    let mut sample =
                        tokio::task::spawn_blocking(move || collect_local(gpu_enabled))
                            .await
                            .unwrap_or_default();
                    if gpu_enabled
                        && let Ok(body) = system_command::run(
                            SystemCommand::NvidiaMetrics,
                            Duration::from_secs(2),
                        )
                        .await
                        && let Some(mut nvidia) = parse_nvidia_metrics(&body)
                    {
                        let devices = sample.gpus.get_or_insert_with(Vec::new);
                        nvidia.truncate(MAX_GPU_DEVICES.saturating_sub(devices.len()));
                        devices.extend(nvidia);
                    }
                    sender.send_replace(Some((Instant::now(), sample)));
                    tokio::time::sleep(REFRESH_INTERVAL).await;
                }
            })
        });
        Self { latest, worker }
    }

    pub fn current(&self) -> ExtendedSnapshot {
        self.latest
            .borrow()
            .as_ref()
            .filter(|(collected_at, _)| collected_at.elapsed() <= MAX_SAMPLE_AGE)
            .map_or_else(ExtendedSnapshot::default, |(_, sample)| sample.clone())
    }
}

impl Drop for ExtendedCollector {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_local(gpu_enabled: bool) -> ExtendedSnapshot {
    let started = Instant::now();
    ExtendedSnapshot {
        process_count: count_processes(Path::new("/proc"), started),
        tcp_connection_count: count_sockets("/proc/net/tcp", "/proc/net/tcp6", started),
        udp_connection_count: count_sockets("/proc/net/udp", "/proc/net/udp6", started),
        gpus: gpu_enabled.then(collect_amd_gpus).flatten(),
    }
}

#[cfg(not(target_os = "linux"))]
fn collect_local(_gpu_enabled: bool) -> ExtendedSnapshot {
    ExtendedSnapshot::default()
}

#[cfg(target_os = "linux")]
fn count_processes(root: &Path, started: Instant) -> Option<u64> {
    let mut count = 0_u64;
    for (index, entry) in fs::read_dir(root).ok()?.enumerate() {
        if index >= MAX_PROCESS_ENTRIES || started.elapsed() > COUNTER_BUDGET {
            return None;
        }
        let entry = entry.ok()?;
        let name = entry.file_name();
        if !name.is_empty()
            && name.as_encoded_bytes().iter().all(u8::is_ascii_digit)
            && entry.file_type().ok()?.is_dir()
        {
            count += 1;
        }
    }
    Some(count)
}

#[cfg(target_os = "linux")]
fn count_sockets(ipv4: &str, ipv6: &str, started: Instant) -> Option<u64> {
    let v4 = count_socket_table(fs::File::open(ipv4).ok()?, started)?;
    let v6 = match fs::File::open(ipv6) {
        Ok(file) => count_socket_table(file, started)?,
        // A kernel built without IPv6 legitimately has no IPv6 socket table.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => return None,
    };
    v4.checked_add(v6)
}

#[cfg(any(target_os = "linux", test))]
// Each scan is bounded to an 8 KiB buffer; a SIMD counting dependency is unnecessary.
#[allow(clippy::naive_bytecount)]
fn count_socket_table(mut reader: impl Read, started: Instant) -> Option<u64> {
    let mut buffer = [0_u8; 8192];
    let mut bytes = 0_usize;
    let mut lines = 0_u64;
    let mut last_byte = None;
    loop {
        if started.elapsed() > COUNTER_BUDGET {
            return None;
        }
        let read = reader.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        bytes = bytes.checked_add(read)?;
        if bytes > MAX_PROC_BYTES {
            return None;
        }
        lines += buffer[..read].iter().filter(|byte| **byte == b'\n').count() as u64;
        last_byte = buffer.get(read - 1).copied();
    }
    if last_byte.is_some_and(|byte| byte != b'\n') {
        lines += 1;
    }
    // Every proc socket table includes its column header. An empty or unreadable
    // table is an unavailable capability, not a confirmed zero-socket sample.
    lines.checked_sub(1)
}

#[cfg(target_os = "linux")]
fn small_file(path: impl AsRef<Path>) -> Option<String> {
    let mut body = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_SMALL_FILE_BYTES + 1)
        .read_to_string(&mut body)
        .ok()?;
    if body.len() as u64 > MAX_SMALL_FILE_BYTES {
        return None;
    }
    Some(body.trim().to_owned())
}

fn finite_range(value: &str, minimum: f32, maximum: f32) -> Option<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite() && (minimum..=maximum).contains(number))
}

#[cfg(target_os = "linux")]
fn collect_amd_gpus() -> Option<Vec<GpuSnapshot>> {
    let mut devices = Vec::new();
    for entry in fs::read_dir("/sys/class/drm").ok()?.take(128) {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let Some(index) = name.to_str().and_then(|name| name.strip_prefix("card")) else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let path = entry.path().join("device");
        if small_file(path.join("vendor")).as_deref() != Some("0x1002") {
            continue;
        }
        let usage_percent = small_file(path.join("gpu_busy_percent"))
            .and_then(|value| finite_range(&value, 0.0, 100.0));
        let memory_total_bytes =
            small_file(path.join("mem_info_vram_total")).and_then(|value| value.parse().ok());
        let memory_used_bytes =
            small_file(path.join("mem_info_vram_used")).and_then(|value| value.parse().ok());
        let temperature_celsius = fs::read_dir(path.join("hwmon")).ok().and_then(|sensors| {
            sensors.take(16).find_map(|sensor| {
                small_file(sensor.ok()?.path().join("temp1_input"))
                    .and_then(|value| finite_range(&value, -100_000.0, 200_000.0))
                    .map(|value| value / 1000.0)
            })
        });
        if usage_percent.is_none()
            && memory_total_bytes.is_none()
            && memory_used_bytes.is_none()
            && temperature_celsius.is_none()
        {
            continue;
        }
        devices.push(GpuSnapshot {
            name: format!("AMD GPU {index}"),
            usage_percent,
            memory_total_bytes,
            memory_used_bytes,
            temperature_celsius,
        });
        if devices.len() == MAX_GPU_DEVICES {
            break;
        }
    }
    (!devices.is_empty()).then_some(devices)
}

fn parse_nvidia_metrics(body: &[u8]) -> Option<Vec<GpuSnapshot>> {
    let text = std::str::from_utf8(body).ok()?;
    let mut devices = Vec::new();
    for row in text.lines().take(MAX_GPU_DEVICES) {
        let fields: Vec<_> = row.split(',').map(str::trim).take(6).collect();
        if fields.len() != 5
            || fields[0].is_empty()
            || fields[0].len() > 128
            || fields[0].chars().any(char::is_control)
        {
            return None;
        }
        let memory = |value: &str| value.parse::<u64>().ok()?.checked_mul(1024 * 1024);
        devices.push(GpuSnapshot {
            name: fields[0].to_owned(),
            usage_percent: finite_range(fields[1], 0.0, 100.0),
            memory_total_bytes: memory(fields[2]),
            memory_used_bytes: memory(fields[3]),
            temperature_celsius: finite_range(fields[4], -100.0, 200.0),
        });
    }
    (!devices.is_empty()).then_some(devices)
}

/// A handful of bounded Linux metadata reads; explicit local configuration wins.
/// Empty means unknown. We never label an unrecognized machine as bare metal.
pub fn detect_virtualization() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        for (path, label) in [("/.dockerenv", "docker"), ("/run/.containerenv", "podman")] {
            if Path::new(path).is_file() {
                return Some(label.to_owned());
            }
        }
        if let Some(container) = small_file("/run/systemd/container") {
            if matches!(
                container.as_str(),
                "docker" | "podman" | "lxc" | "lxc-libvirt" | "systemd-nspawn" | "openvz"
            ) {
                return Some(container);
            }
        }
        let product = small_file("/sys/class/dmi/id/product_name").unwrap_or_default();
        let vendor = small_file("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
        let hypervisor = small_file("/sys/hypervisor/type").unwrap_or_default();
        let release = small_file("/proc/sys/kernel/osrelease").unwrap_or_default();
        virtualization_from_metadata(&format!("{product} {vendor} {hypervisor} {release}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn virtualization_from_metadata(metadata: &str) -> Option<String> {
    let metadata = metadata.to_ascii_lowercase();
    for (pattern, label) in [
        ("microsoft-standard", "wsl"),
        ("vmware", "vmware"),
        ("virtualbox", "virtualbox"),
        ("innotek", "virtualbox"),
        ("kvm", "kvm"),
        ("qemu", "qemu"),
        ("xen", "xen"),
        ("amazon ec2", "amazon-ec2"),
        ("google compute engine", "google-compute"),
        ("parallels", "parallels"),
    ] {
        if metadata.contains(pattern) {
            return Some(label.to_owned());
        }
    }
    if metadata.contains("microsoft corporation") && metadata.contains("virtual machine") {
        return Some("hyper-v".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_socket_rows_and_distinguishes_missing_data() {
        assert_eq!(
            count_socket_table(&b"header\nrow1\nrow2\n"[..], Instant::now()),
            Some(2)
        );
        assert_eq!(
            count_socket_table(&b"header\n"[..], Instant::now()),
            Some(0)
        );
        assert_eq!(count_socket_table(&b""[..], Instant::now()), None);
        assert_eq!(
            count_socket_table(
                std::io::repeat(b'x').take((MAX_PROC_BYTES + 1) as u64),
                Instant::now()
            ),
            None
        );
    }

    #[test]
    fn unsupported_gpu_values_remain_null() {
        let devices = parse_nvidia_metrics(
            b"NVIDIA Example, 13, 8192, 1024, 55\nNVIDIA Other, [N/A], [N/A], [N/A], [N/A]\n",
        )
        .unwrap();
        assert_eq!(devices[0].memory_total_bytes, Some(8192 * 1024 * 1024));
        assert_eq!(devices[0].temperature_celsius, Some(55.0));
        assert_eq!(devices[1].usage_percent, None);
        assert_eq!(devices[1].memory_used_bytes, None);
        assert_eq!(devices[1].temperature_celsius, None);
        assert!(parse_nvidia_metrics(b"bad row").is_none());
        assert_eq!(finite_range("NaN", 0.0, 100.0), None);
    }

    #[test]
    fn virtualization_detection_does_not_guess_bare_metal() {
        assert_eq!(virtualization_from_metadata("Dell PowerEdge"), None);
        assert_eq!(
            virtualization_from_metadata("KVM QEMU"),
            Some("kvm".to_owned())
        );
        assert_eq!(
            virtualization_from_metadata("Virtual Machine Microsoft Corporation"),
            Some("hyper-v".to_owned())
        );
    }

    #[test]
    fn disabled_collector_needs_no_runtime_and_has_no_worker() {
        let collector = ExtendedCollector::start(false, false);
        assert!(collector.worker.is_none());
        assert!(collector.current().process_count.is_none());
    }
}
