# syntax=docker/dockerfile:1.7
# Targets: `server` (default, last stage) and `client`.

# ---- admin UI (Vike prerendered static files) --------------------------------
FROM node:24-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ---- Rust binaries ----------------------------------------------------------
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p exposr-server -p exposr-client \
    && mkdir -p /out && cp target/release/exposr-server target/release/exposr-client /out/

# ---- client image -------------------------------------------------------------
FROM debian:bookworm-slim AS client
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* && useradd -r -u 10001 exposr
COPY --from=build /out/exposr-client /usr/local/bin/exposr-client
USER exposr
ENTRYPOINT ["exposr-client"]

# ---- server image (default) ---------------------------------------------------
FROM debian:bookworm-slim AS server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 exposr && mkdir -p /data && chown exposr /data
COPY --from=build /out/exposr-server /usr/local/bin/exposr-server
COPY --from=build /out/exposr-client /usr/local/bin/exposr-client
COPY --from=web /web/dist/client /app/web
ENV WEB_DIR=/app/web \
    DATA_DIR=/data \
    ADMIN_ADDR=0.0.0.0:8080 \
    CONTROL_ADDR=0.0.0.0:2333 \
    RUST_LOG=info
USER exposr
VOLUME /data
EXPOSE 8080 2333
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s \
    CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["exposr-server"]
