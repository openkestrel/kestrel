# syntax=docker/dockerfile:1

# rust:1.96.0-slim-trixie
FROM rust@sha256:c37af730be4fd8104cbf9aedbd6ab259e51ca2d5437817a0f8680edf66ac6c28 AS build

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release --package kestrel

# oven/bun:1.3.14-slim
FROM oven/bun@sha256:d56a2534ffd262e92c12fd3249d3924d296d97086da773f821d7d0477435ea04 AS client

WORKDIR /src
COPY package.json bun.lock ./
COPY packages/client/package.json ./packages/client/
RUN bun install --frozen-lockfile --filter @kestrel/client
COPY openapi/operator.json ./openapi/
COPY packages/client ./packages/client
RUN cd packages/client && bun --bun run build

# debian:trixie-slim
FROM debian@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

# The Docker driver provisions an Environment by executing `docker`, so this image carries a
# client whatever kestrel is linked against (ADR-0008). `curl` is how a healthcheck asks the
# operator boundary, since the image carries no kestrel Client to ask it with (ADR-0015).
# `tzdata` is where a cron Trigger's time zone is read from.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl docker-cli tzdata \
    && rm --recursive --force /var/lib/apt/lists/* \
    && useradd --create-home kestrel \
    && install --directory --owner kestrel --group kestrel /var/lib/kestrel

COPY --from=build /src/target/release/kestrel-control-plane /usr/local/bin/kestrel-control-plane
COPY --from=client /src/packages/client/dist/client /usr/share/kestrel/client

ENV HOME=/home/kestrel
ENV KESTREL_DATA_DIR=/var/lib/kestrel
ENV KESTREL_CLIENT_DIR=/usr/share/kestrel/client
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
