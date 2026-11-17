FROM rust:1.91-bookworm AS server-builder
WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates

RUN cargo build --release -p open-comms-server

FROM node:22-alpine AS web-builder
WORKDIR /app

COPY apps/web ./
RUN npm ci --no-audit --no-fund && npm run build

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=server-builder /app/target/release/open-comms-server /usr/local/bin/open-comms-server
COPY --from=web-builder /app/dist /app/web

ENV APP_BIND_ADDR=0.0.0.0:8080
ENV WEB_DIST_DIR=/app/web
EXPOSE 8080
CMD ["open-comms-server"]
