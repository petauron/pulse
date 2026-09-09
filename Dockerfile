# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

FROM node:24-bookworm-slim@sha256:ba849c60be29959425b8734d57b8b4b7d56f98edd9504c9af091d5281095a71e AS web-builder
WORKDIR /src/web
ARG PULSE_BUILD_GIT_HASH=unknown
ENV PULSE_BUILD_GIT_HASH=${PULSE_BUILD_GIT_HASH}
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS rust-builder
WORKDIR /src
ARG PULSE_BUILD_GIT_HASH=unknown
ENV PULSE_BUILD_GIT_HASH=${PULSE_BUILD_GIT_HASH} \
    RUSTUP_TOOLCHAIN=1.98.0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY --from=web-builder /src/web/dist web/dist
RUN cargo build --locked --release -p pulse-service -p pulse-agent

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime
ARG PULSE_BUILD_GIT_HASH=unknown
LABEL org.opencontainers.image.source="https://github.com/petauron/pulse" \
      org.opencontainers.image.description="Self-hosted node monitoring with an outbound-only Agent" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.revision="${PULSE_BUILD_GIT_HASH}"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 pulse \
    && useradd --uid 65532 --gid pulse --system --home-dir /var/lib/pulse --shell /usr/sbin/nologin pulse \
    && install --directory --owner pulse --group pulse --mode 0700 /var/lib/pulse
COPY --from=rust-builder /src/target/release/pulse-service /usr/local/bin/pulse-service
COPY --from=rust-builder /src/target/release/pulse-agent /usr/local/bin/pulse-agent
COPY LICENSE NOTICE RUST_THIRD_PARTY_LICENSES.html RUST_STDLIB_LICENSES.html /usr/share/doc/pulse/
COPY web/THIRD_PARTY_NOTICES.md web/LICENSE.* /usr/share/doc/pulse/web/
COPY --from=web-builder /src/web/dist/THIRD_PARTY_LICENSES.md /usr/share/doc/pulse/web/
USER 65532:65532
WORKDIR /var/lib/pulse
ENV PULSE_LISTEN=0.0.0.0:8080 \
    PULSE_ALLOW_PUBLIC_LISTEN=true \
    PULSE_DATABASE_PATH=/var/lib/pulse/pulse.db
VOLUME ["/var/lib/pulse"]
EXPOSE 8080
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/pulse-service"]
CMD ["serve"]
