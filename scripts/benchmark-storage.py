#!/usr/bin/env python3
"""Compare two real Service binaries using disposable loopback-only databases.

No Agents, production data or remote hosts are used. Measures OS process write
accounting (not SSD wear). Keep identical build profiles for both binaries.
"""
import argparse
import ctypes
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid


class RusageV2(ctypes.Structure):
    # macOS SDK sys/resource.h, struct rusage_info_v2 (flavor 2).
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "user", "system", "idle", "interrupt", "pageins", "wired",
            "resident", "footprint", "start", "exit", "child_user",
            "child_system", "child_idle", "child_interrupt", "child_pageins",
            "child_elapsed", "read_bytes", "write_bytes",
        )
    ]


def written_bytes(pid):
    if sys.platform == "darwin":
        library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        library.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        library.proc_pid_rusage.restype = ctypes.c_int
        usage = RusageV2()
        if library.proc_pid_rusage(pid, 2, ctypes.byref(usage)):
            raise OSError(ctypes.get_errno(), "cannot read benchmark process I/O")
        return usage.write_bytes
    values = dict(line.split(":", 1) for line in Path(f"/proc/{pid}/io").read_text().splitlines())
    return int(values["write_bytes"])


def emit(**data):
    print(json.dumps(data), flush=True)


class Service:
    def __init__(self, name, binary, directory, interval, expected_schema):
        self.name, self.directory, self.interval = name, directory, interval
        self.process = None
        self.count = 0
        self.directory.mkdir(mode=0o700)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        self.base = f"http://127.0.0.1:{port}"
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(self.jar))
        bootstrap = secrets.token_hex(32)
        token_path = directory / "setup-token"
        with os.fdopen(os.open(token_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as stream:
            stream.write(bootstrap)
        environment = {k: v for k, v in os.environ.items() if not k.startswith("PULSE_")}
        environment.update({
            "PULSE_DATABASE_PATH": str(directory / "pulse.db"),
            "PULSE_LISTEN": f"127.0.0.1:{port}", "PULSE_PUBLIC_URL": self.base,
            "PULSE_SETUP_TOKEN_FILE": str(token_path), "PULSE_MAX_NODES": "10",
            "PULSE_MAX_DATABASE_BYTES": "67108864", "RUST_LOG": "error",
        })
        self.environment = environment
        enrollment = subprocess.run([str(binary), "enrollment", "create", "--ttl-seconds", "600"],
                                    env=environment, cwd=directory, capture_output=True, check=True, timeout=20)
        secret = json.loads(enrollment.stdout)["token"]
        self.process = subprocess.Popen([str(binary), "serve"], env=environment, cwd=directory,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                if self.process.poll() is not None:
                    raise RuntimeError(f"{name}: Service exited during startup")
                try:
                    status = self.call("/api/auth/status")
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError(f"{name}: startup timed out")
            self.call("/api/auth/setup", {"token": bootstrap, "username": "bench", "password": "Test9" + secrets.token_hex(24)}, csrf=status["csrf_token"])
            credentials = self.call("/api/v1/agents/enroll", {
                "protocol_version": 2, "node_name": "storage-bench", "agent_version": "bench",
                "region": "SG", "group": "test",
            }, token=secret)
            self.token, self.node = credentials["agent_token"], credentials["node_id"]
            with sqlite3.connect(f"file:{directory / 'pulse.db'}?mode=ro", uri=True) as db:
                schema = db.execute("PRAGMA user_version").fetchone()[0]
            if schema != expected_schema:
                raise RuntimeError(f"{name}: expected schema {expected_schema}, got {schema}; check build artifacts")
        except BaseException:
            self.close()
            raise

    def call(self, path, body=None, token=None, csrf=None):
        headers = {"Content-Type": "application/json", "Origin": self.base}
        if token:
            headers["Authorization"] = "Bearer " + token
        if csrf:
            headers["X-CSRF-Token"] = csrf
        request = urllib.request.Request(self.base + path,
            json.dumps(body).encode() if body is not None else None, headers)
        with self.http.open(request, timeout=10) as response:
            return json.loads(response.read())

    def submit(self):
        index = self.count + 1
        self.last = {
            "protocol_version": 2, "sample_id": str(uuid.UUID(int=index)),
            "collected_at_unix_ms": time.time_ns() // 1_000_000,
            "host_name": "storage-bench", "agent_version": "bench", "operating_system": "Linux",
            "kernel_version": "6.0", "architecture": "x86_64", "cpu_name": "Test CPU",
            "cpu_cores": 2, "virtualization": "kvm", "region": "SG", "group": "test",
            "uptime_seconds": 100 + index, "cpu_usage_percent": 12.5 + index % 5,
            "load_one": 0.1, "load_five": 0.2, "load_fifteen": 0.3,
            "memory_total_bytes": 1073741824, "memory_used_bytes": 536870912 + index,
            "swap_total_bytes": 268435456, "swap_used_bytes": 0,
            "disk_total_bytes": 4294967296, "disk_used_bytes": 1073741824,
            "network_receive_bytes_per_second": 100, "network_transmit_bytes_per_second": 200,
            "network_total_received_bytes": index * 100, "network_total_transmitted_bytes": index * 200,
            "process_count": None, "tcp_connection_count": None, "udp_connection_count": None, "gpus": None,
        }
        response = self.call("/api/v1/agents/snapshots", self.last, token=self.token)
        assert response["accepted"] is True
        self.count = index

    def verify(self):
        assert self.call("/api/v1/agents/snapshots", self.last, token=self.token)["accepted"] is True
        history = self.call(f"/api/v1/nodes/{self.node}/history?hours=1&limit=1000")
        assert history["coverage"]["source_points"] == self.count
        if self.count <= 1000:
            assert len(history["records"]) == self.count
        else:
            assert 0 < len(history["records"]) <= 1000
        nodes = self.call("/api/v1/nodes?limit=10")
        assert nodes["nodes"][0]["status"]["online"] is True

    def close(self, crash=False):
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGKILL if crash else signal.SIGTERM)
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)

    def durable_count(self):
        metrics = self.directory / "pulse.metrics.db"
        database = metrics if metrics.exists() else self.directory / "pulse.db"
        with sqlite3.connect(database) as db:
            assert db.execute("PRAGMA integrity_check(1)").fetchone()[0] == "ok"
            count = db.execute("SELECT count(*) FROM snapshots").fetchone()[0]
        assert count == self.count, "acknowledged samples missing after process crash"
        return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--duration", type=int, default=360)
    parser.add_argument("--upgrade-only", action="store_true", help="verify a real stopped-Service upgrade instead of measuring writes")
    args = parser.parse_args()
    if not 30 <= args.duration <= 1800:
        parser.error("duration must be 30..1800 seconds")
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    emit(event="binaries", hashes={k: hashlib.sha256(p.read_bytes()).hexdigest() for k, p in binaries.items()}, platform=sys.platform)
    if args.upgrade_only:
        verify_upgrade(binaries)
        return
    services = []
    with tempfile.TemporaryDirectory(prefix="pulse-storage-benchmark-") as root:
        try:
            for interval in [1, 3]:
                for profile, binary in binaries.items():
                    name = f"{profile}-{interval}s"
                    services.append(Service(name, binary, Path(root) / name, interval, 3 if profile == "baseline" else 4))
            for _ in range(3):
                for service in services:
                    service.submit()
                time.sleep(3)
            start = time.monotonic()
            initial = {s.name: (written_bytes(s.process.pid), s.count) for s in services}
            due = {s.name: start for s in services}
            progress = start
            while time.monotonic() - start < args.duration:
                for service in services:
                    if time.monotonic() >= due[service.name]:
                        service.submit()
                        due[service.name] = time.monotonic() + service.interval
                if time.monotonic() >= progress:
                    emit(event="progress", elapsed_seconds=round(time.monotonic() - start),
                         samples={s.name: s.count - initial[s.name][1] for s in services})
                    progress = time.monotonic() + 15
                time.sleep(0.005)
            results = [{"profile": s.name, "samples": s.count - initial[s.name][1],
                        "write_bytes": written_bytes(s.process.pid) - initial[s.name][0]} for s in services]
            for service in services:
                service.verify()
                service.close(crash=True)
                count = service.durable_count()
                emit(event="verified", profile=service.name, persisted_after_sigkill=count)
            emit(event="result", duration_seconds=args.duration, measurements=results,
                 note="OS process write accounting; local debug Service builds, synthetic HTTP samples, not Agent CPU/RAM or SSD wear")
        finally:
            for service in services:
                service.close()
    emit(event="cleanup", temporary_databases_removed=True, service_processes_stopped=True)


def verify_upgrade(binaries):
    with tempfile.TemporaryDirectory(prefix="pulse-storage-upgrade-") as root:
        service = Service("upgrade", binaries["baseline"], Path(root) / "state", 1, 3)
        try:
            for _ in range(3):
                service.submit()
                time.sleep(1)
            service.verify()
            # Leave old-version WAL behind: migration/backup must recover it.
            assert (service.directory / "pulse.db-wal").stat().st_size > 32
            service.close(crash=True)
            service.process = subprocess.Popen([str(binaries["candidate"]), "serve"],
                env=service.environment, cwd=service.directory,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                if service.process.poll() is not None:
                    raise RuntimeError("upgraded Service exited during migration")
                try:
                    service.call("/healthz")
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("upgraded Service startup timed out")
            # Existing cookie, Agent credential, IDs and raw rows must still work.
            service.verify()
            service.submit()
            service.verify()
            backups = list(service.directory.glob("pulse.db.backup-*"))
            assert len(backups) == 1 and backups[0].is_file()
            db = sqlite3.connect(f"file:{backups[0]}?mode=ro", uri=True)
            try:
                assert db.execute("PRAGMA user_version").fetchone()[0] == 3
                assert db.execute("SELECT count(*) FROM snapshots").fetchone()[0] == 3
            finally:
                db.close()
            result = subprocess.run([str(binaries["candidate"]), "backup"], env=service.environment,
                cwd=service.directory, capture_output=True, check=True, timeout=20)
            paired = Path(result.stdout.decode().strip())
            assert paired.parent == service.directory and paired.is_dir()
            assert {"pulse.db", "pulse.metrics.db", "manifest.json"} <= {p.name for p in paired.iterdir()}
            service.close(crash=True)
            assert service.durable_count() == 4
            emit(event="upgrade_verified", old_schema=3, new_schema=4,
                 original_session_valid=True, original_agent_credential_valid=True,
                 pre_upgrade_wal_backed_up=True, paired_cli_backup_valid=True,
                 acknowledged_samples_preserved=4)
        finally:
            service.close()
    emit(event="cleanup", temporary_databases_removed=True, service_processes_stopped=True)


if __name__ == "__main__":
    main()
