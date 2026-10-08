# syntax=docker/dockerfile:1

# rust:1.96.0-slim-trixie
FROM rust@sha256:c37af730be4fd8104cbf9aedbd6ab259e51ca2d5437817a0f8680edf66ac6c28 AS build

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release --package kestrel --package kestrel-client

# debian:trixie-slim
FROM debian@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

# The Docker driver provisions an Environment by executing `docker`, so this image carries a
# client whatever kestrel is linked against (ADR-0008); `curl` serves the healthcheck, `kestrel`
# an operator beside the database, and `tzdata` a cron Trigger's time zone.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl docker-cli tzdata \
    && rm --recursive --force /var/lib/apt/lists/* \
    && useradd --create-home kestrel \
    && install --directory --owner kestrel --group kestrel /var/lib/kestrel

COPY --from=build /src/target/release/kestrel-control-plane /usr/local/bin/kestrel-control-plane
COPY --from=build /src/target/release/kestrel /usr/local/bin/kestrel

ENV HOME=/home/kestrel
ENV KESTREL_DATA_DIR=/var/lib/kestrel
# Loopback is the right default for a binary on a laptop and unreachable in a container: what
# dials the link is an Environment outside this one (ADR-0002), and what reaches the operator
# boundary is a Client outside it. The operator boundary authenticates nobody, so publish it
# on the host's loopback and nowhere else (ADR-0015).
ENV KESTREL_LISTEN=0.0.0.0:7717
ENV KESTREL_OPERATOR_LISTEN=0.0.0.0:7718

VOLUME /var/lib/kestrel
EXPOSE 7717 7718
USER kestrel
WORKDIR /var/lib/kestrel

ENTRYPOINT ["kestrel-control-plane"]
