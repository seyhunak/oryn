# ORYN control plane (oryn-api). Multi-stage: the builder caches dependencies via
# BuildKit cache mounts; the runtime image has no compiler, runs as a non-root user and
# includes git (repository imports shell out to `git clone` with hooks disabled).
# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock clippy.toml ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p oryn-api -p oryn-db --bins \
 && cp target/release/oryn-api target/release/oryn-migrate /

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data oryn \
 && mkdir -p /data/artifacts /data/repos && chown -R oryn /data
COPY --from=build /oryn-api /oryn-migrate /usr/local/bin/
USER oryn
ENV ORYN_BIND=0.0.0.0:8080 ORYN_ARTIFACT_DIR=/data/artifacts ORYN_WORKSPACE_DIR=/data/repos
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=5 CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/oryn-api"]
