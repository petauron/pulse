#!/bin/sh
set -eu

# CI and release archives must use the same libc baseline as the Service image.
# Bind mounts require a local Linux Docker daemon, not a remote desktop context.
[ "$(uname -s)" = Linux ] || { echo "requires a local Linux Docker runner" >&2; exit 1; }
: "${PULSE_BUILD_GIT_HASH:?set the source revision}"
: "${PULSE_BUILD_TARGET:?set the native Rust target}"
repository=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
builder=rust:1.98-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922
runtime=debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171

docker run --rm \
  --env PULSE_BUILD_GIT_HASH --env PULSE_BUILD_TARGET \
  --env CARGO_INCREMENTAL=0 --env RUSTUP_TOOLCHAIN=1.98.0 \
  --mount "type=bind,src=$repository,dst=/src" --workdir /src \
  "$builder" sh -c '
    set -eu
    test "$(rustc -vV | sed -n "s/^host: //p")" = "$PULSE_BUILD_TARGET"
    cargo build --locked --release -p pulse-service -p pulse-agent
  '

# A successful link on a newer runner does not prove compatibility with users.
docker run --rm --network none --read-only --user 65532:65532 \
  --cap-drop ALL --memory 192m --pids-limit 64 \
  --mount "type=bind,src=$repository/target/release,dst=/opt/pulse,readonly" \
  "$runtime" sh -c '
    set -eu
    /opt/pulse/pulse-service --version
    /opt/pulse/pulse-agent --version
  '
