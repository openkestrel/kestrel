# syntax=docker/dockerfile:1

# debian:trixie-slim
FROM debian@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS opencode

ARG TARGETARCH
ARG OPENCODE_VERSION=2.0.14
ARG OPENCODE_SHA256_AMD64=0da3246c6d22f393739bb85d788055c16e381635cf0c1ceed393c28fd13ea964
ARG OPENCODE_SHA256_ARM64=c76b4f9490f3d943f0ab9f22707a94082a27ad062abda3da41d8c61ea4b04c04

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm --recursive --force /var/lib/apt/lists/*

# There is no checksums file on the standalone channel; upstream's update API maps each artifact
# to a URL and a SHA-256, and one is recorded here so the build fails rather than shipping
# whatever the CDN served.
RUN set -eu; \
    case "${TARGETARCH}" in \
    amd64) release=x64-baseline; digest="${OPENCODE_SHA256_AMD64}" ;; \
    arm64) release=arm64; digest="${OPENCODE_SHA256_ARM64}" ;; \
    *) echo "kestrel-env is not built for ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    url="https://opencode.ai/files/bin/${OPENCODE_VERSION}/opencode-linux-${release}.tar.gz"; \
    curl --fail --silent --show-error --location --output /tmp/opencode.tar.gz "${url}"; \
    echo "${digest}  /tmp/opencode.tar.gz" | sha256sum --check --strict -; \
    tar --extract --gzip --file /tmp/opencode.tar.gz --directory /usr/local/bin opencode; \
    rm /tmp/opencode.tar.gz

# node:24-trixie-slim
FROM node@sha256:50c3b2f6988dfc307b86e5301d69611af31f4789bdf232863b07d3b02fe55ae0 AS adapters

WORKDIR /opt/acp
COPY images/kestrel-env/package.json images/kestrel-env/package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts
# The wrapper's postinstall copies the native `claude` binary over a stub that --ignore-scripts
# would otherwise leave behind, so that one install step runs explicitly and nothing else does.
RUN node ./node_modules/@anthropic-ai/claude-code/install.cjs
# Every catalogue command and both sign-in subcommands run with no credentials and no sign-in.
RUN ./node_modules/.bin/claude-agent-acp --version \
    && ./node_modules/.bin/codex-acp --version \
    && ./node_modules/.bin/codex --version \
    && ./node_modules/.bin/claude --version \
    && ./node_modules/.bin/codex login --help \
    && ./node_modules/.bin/claude setup-token --help

# rust:1.96.0-slim-trixie
FROM rust@sha256:c37af730be4fd8104cbf9aedbd6ab259e51ca2d5437817a0f8680edf66ac6c28 AS supervisor

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release --package kestrel-supervisor

# debian:trixie-slim
FROM debian@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates git \
    && rm --recursive --force /var/lib/apt/lists/* \
    && useradd --create-home kestrel \
    && install --directory --owner kestrel --group kestrel /workspace

COPY --from=opencode /usr/local/bin/opencode /usr/local/bin/opencode
COPY --from=supervisor /src/target/release/kestrel-supervisor /usr/local/bin/kestrel-supervisor
COPY --from=adapters /usr/local/bin/node /usr/local/bin/node
COPY --from=adapters /opt/acp /opt/acp
RUN ln --symbolic /opt/acp/node_modules/.bin/claude-agent-acp /usr/local/bin/claude-agent-acp \
    && ln --symbolic /opt/acp/node_modules/.bin/codex-acp /usr/local/bin/codex-acp \
    && ln --symbolic /opt/acp/node_modules/.bin/claude /usr/local/bin/claude \
    && ln --symbolic /opt/acp/node_modules/.bin/codex /usr/local/bin/codex

# The catalogue names these harnesses; a derived image inherits the label and overrides it when
# it carries a different set (ADR-0048).
LABEL dev.kestrel.harnesses="opencode,claude,codex"

# Docker leaves HOME unset for anyone but root, and a harness with nowhere to keep its
# state writes to `/`.
# A pinned image must not move off its pin, and the credential database keeps one filename
# across release channels.
ENV HOME=/home/kestrel \
    OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_DISABLE_CHANNEL_DB=1
USER kestrel
WORKDIR /workspace

# The Session's process, not a process that starts the Session's process: nothing here outlives the
# supervisor to restart it (ADR-0007).
ENTRYPOINT ["kestrel-supervisor"]
