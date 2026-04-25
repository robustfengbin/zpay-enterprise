# ==============================================================================
# zpay-enterprise backend — multi-stage Dockerfile
#
# Builds the Rust backend only. The React frontend is packaged separately;
# serve it from any static host (nginx, Cloudflare Pages, etc.) or build
# it with `cd frontend && npm ci && npm run build` and serve the resulting
# frontend/dist directory.
#
# Per project CLAUDE.md rule 1: we build in debug mode (no --release).
# ==============================================================================

# ---- Stage 1: build Rust backend ---------------------------------------------
FROM rust:1.90-bookworm AS backend-builder

WORKDIR /build

# System deps required by sqlx (mysql), openssl, zcash/orchard crates
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY backend/ ./backend/

WORKDIR /build/backend
RUN cargo build

# ---- Stage 2: runtime --------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd -g 10001 zpay \
 && useradd  -u 10001 -g zpay -d /app -s /sbin/nologin zpay

WORKDIR /app

# Copy the debug binary per CLAUDE.md rule 1
COPY --from=backend-builder /build/backend/target/debug/web3_wallet_service /app/web3_wallet_service

# Logs directory (main.rs creates it at startup but we pre-own it)
RUN mkdir -p /app/logs && chown -R zpay:zpay /app
USER zpay

EXPOSE 8080

# Config is supplied via WEB3_* environment variables — see .env.example.
CMD ["/app/web3_wallet_service"]
