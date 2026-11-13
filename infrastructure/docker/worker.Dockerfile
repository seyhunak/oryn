# ORYN execution worker. Stateless apart from its persisted identity token
# (ORYN_WORKER_TOKEN_FILE). Browser jobs are delegated to a Playwright runner over HTTP.
# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock clippy.toml ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p oryn-worker && cp target/release/oryn-worker /oryn-worker

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /state oryn \
 && mkdir -p /state && chown oryn /state
COPY --from=build /oryn-worker /usr/local/bin/oryn-worker
USER oryn
ENV ORYN_WORKER_TOKEN_FILE=/state/worker-token
ENTRYPOINT ["/usr/local/bin/oryn-worker"]
