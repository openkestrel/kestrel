# syntax=docker/dockerfile:1

ARG KESTREL_ENV=kestrel-env

# rust:1.96.0-slim-trixie, the toolchain rust-toolchain.toml names.
FROM rust@sha256:c37af730be4fd8104cbf9aedbd6ab259e51ca2d5437817a0f8680edf66ac6c28 AS rust

RUN rustup component add rustfmt clippy

FROM ${KESTREL_ENV}

USER root

# The linker cargo drives, and the C compiler ring, aws-lc-sys and libsqlite3-sys build with.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends gcc libc6-dev \
    && rm --recursive --force /var/lib/apt/lists/*

# Owned by the agent's user, because cargo writes its registry and git caches under CARGO_HOME.
COPY --from=rust --chown=kestrel:kestrel /usr/local/rustup /usr/local/rustup
COPY --from=rust --chown=kestrel:kestrel /usr/local/cargo /usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
# Codex runs each command in a login shell, whose /etc/profile resets PATH without cargo.
RUN echo 'export PATH=/usr/local/cargo/bin:$PATH' > /etc/profile.d/cargo.sh

USER kestrel
