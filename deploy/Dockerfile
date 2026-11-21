# Multi-stage build for the ff-api axum server.
# Build context is the workspace root (see deploy/README.md).
# Pinned toolchain. rust-toolchain.toml says "stable", which rustup would
# resolve to the newest release, and Rust 1.100 rejects sqlx-postgres
# 0.8.0 (never-type fallback). No sqlx 0.8.x fix resolves alongside
# rusqlite 0.31 (both link sqlite3), so unpin once rusqlite is upgraded
# and sqlx can move to 0.8.6+.
FROM rust:1.96-bookworm AS builder
ENV RUSTUP_TOOLCHAIN=1.96.1
WORKDIR /app
COPY . .
RUN cargo build --release -p ff-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ff-api /usr/local/bin/ff-api
ENV FF_ETL_DATA_DIR=/data
ENV PORT=8080
EXPOSE 8080
CMD ["ff-api"]
