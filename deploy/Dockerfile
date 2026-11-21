# Multi-stage build for the ff-api axum server.
# Build context is the workspace root (see deploy/README.md).
FROM rust:1-bookworm AS builder
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
