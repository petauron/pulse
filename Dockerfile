# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

FROM node:26-bookworm-slim@sha256:86f07bc9c5dce4578cf37e5a418b7bfc7f817cda25cde66e2b66e95ed86c4567 AS web-builder
WORKDIR /src/web
ARG SENULUN_BUILD_GIT_HASH=unknown
ENV SENULUN_BUILD_GIT_HASH=${SENULUN_BUILD_GIT_HASH}
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98-bookworm@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa AS rust-builder
WORKDIR /src
ARG SENULUN_BUILD_GIT_HASH=unknown
ENV SENULUN_BUILD_GIT_HASH=${SENULUN_BUILD_GIT_HASH} \
    RUSTUP_TOOLCHAIN=1.98.0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY --from=web-builder /src/web/dist web/dist
RUN cargo build --locked --release -p senulun-service -p senulun-agent

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
ARG SENULUN_BUILD_GIT_HASH=unknown
LABEL org.opencontainers.image.source="https://github.com/petauron/senulun" \
      org.opencontainers.image.description="Self-hosted node monitoring with an outbound-only Agent" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.revision="${SENULUN_BUILD_GIT_HASH}"
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 senulun \
    && useradd --uid 65532 --gid senulun --system --home-dir /var/lib/senulun --shell /usr/sbin/nologin senulun \
    && install --directory --owner senulun --group senulun --mode 0700 /var/lib/senulun
COPY --from=rust-builder /src/target/release/senulun-service /usr/local/bin/senulun-service
COPY --from=rust-builder /src/target/release/senulun-agent /usr/local/bin/senulun-agent
COPY LICENSE NOTICE RUST_THIRD_PARTY_LICENSES.html RUST_STDLIB_LICENSES.html /usr/share/doc/senulun/
COPY web/THIRD_PARTY_NOTICES.md web/LICENSE.* /usr/share/doc/senulun/web/
COPY --from=web-builder /src/web/dist/THIRD_PARTY_LICENSES.md /usr/share/doc/senulun/web/
USER 65532:65532
WORKDIR /var/lib/senulun
ENV SENULUN_LISTEN=0.0.0.0:8080 \
    SENULUN_ALLOW_PUBLIC_LISTEN=true \
    SENULUN_DATABASE_PATH=/var/lib/senulun/store/senulun.db
VOLUME ["/var/lib/senulun"]
EXPOSE 8080
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/senulun-service"]
CMD ["migrate-and-serve"]
