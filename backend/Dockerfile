# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked && \
    install -m 755 target/release/platform-api /platform-api

FROM debian:bookworm-slim AS runtime
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 platform && \
    useradd --uid 10001 --gid platform --no-create-home platform && \
    mkdir -p /app /data/media && \
    chown -R platform:platform /app /data
WORKDIR /app
COPY --from=build /platform-api /usr/local/bin/platform-api
ENV BIND_ADDRESS=0.0.0.0:8080 \
    MEDIA_STORAGE_DIR=/data/media \
    RUST_LOG=info
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["platform-api"]
