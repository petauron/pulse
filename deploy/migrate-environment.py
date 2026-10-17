#!/usr/bin/env python3
"""Offline one-shot conversion. Never source a shell file or print its values."""
import argparse
import os
from pathlib import Path
import re
import tempfile


def convert(raw: bytes) -> bytes:
    if len(raw) > 65536:
        raise ValueError("environment exceeds migration limit")
    text = raw.decode("utf-8")
    lines, seen = [], set()
    for line in text.splitlines(keepends=True):
        if not line.strip() or line.lstrip().startswith("#"):
            lines.append(line)
            continue
        match = re.fullmatch(r"([A-Z][A-Z0-9_]*)=(.*?)(\r?\n)?", line)
        if not match or "\x00" in line:
            raise ValueError("unsupported environment syntax; source is unchanged")
        key, value, newline = match.groups()
        if key.startswith("PULSE_"):
            key = "SENULUN_" + key[len("PULSE_"):]
        if key in seen:
            raise ValueError("conflicting environment assignments")
        seen.add(key)
        # Do not reinterpret a password, URL, or arbitrary custom setting.
        # Existing explicit file paths stay valid until the operator migrates
        # their owned storage. Identity is never reset to a new default path.
        lines.append(key + "=" + value + (newline or ""))
    return "".join(lines).encode("utf-8")


def migrate(source: Path, target: Path) -> None:
    if not source.is_absolute() or not target.is_absolute() or source == target:
        raise ValueError("select distinct absolute source and destination paths")
    for path in [*source.parents, source, *target.parents, target]:
        if path.is_symlink():
            raise ValueError("refusing symlink in migration paths")
    metadata = source.stat()
    if not source.is_file() or metadata.st_mode & 0o077:
        raise ValueError("source must be a private regular file")
    converted = convert(source.read_bytes())
    # The original remains the exact private backup. Link publishes the fully
    # synced new file without overwriting an existing target, even under races.
    descriptor, temporary = tempfile.mkstemp(prefix=".senulun-env-", dir=target.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            os.fchmod(output.fileno(), 0o600)
            if os.geteuid() == 0:
                os.fchown(output.fileno(), metadata.st_uid, metadata.st_gid)
            output.write(converted)
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, target)
        directory = os.open(target.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        os.unlink(temporary)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("target", type=Path)
    arguments = parser.parse_args()
    try:
        migrate(arguments.source, arguments.target)
    except (OSError, ValueError):
        parser.exit(1, "environment migration failed; original file was preserved\n")
