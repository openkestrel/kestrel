# syntax=docker/dockerfile:1

# oven/bun:1.3.14-slim
FROM oven/bun@sha256:d56a2534ffd262e92c12fd3249d3924d296d97086da773f821d7d0477435ea04 AS build

WORKDIR /src
COPY package.json bun.lock ./
COPY packages/client/package.json ./packages/client/
RUN bun install --frozen-lockfile --filter @kestrel/client
COPY openapi/operator.json ./openapi/
COPY packages/client ./packages/client
RUN cd packages/client && bun --bun run build

# caddy:2.11.4-alpine
FROM caddy@sha256:6aeddd44c3078b0f9a35206472a11420648a79c184603ef95957d0a20044cb2b

COPY images/kestrel-client/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /src/packages/client/dist/client /srv

ENV KESTREL_CONTROL_PLANE=http://kestrel:7718
EXPOSE 8080
USER nobody
