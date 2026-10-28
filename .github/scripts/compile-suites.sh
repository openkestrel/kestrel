#!/usr/bin/env bash
# Packs the heavy suites with the helpers they spawn, so a job that runs one needs no toolchain.
set -euo pipefail

archive=$1
suites=(image docker capability opencode development control_plane compose cleanup)
tests=()
for suite in "${suites[@]}"; do
  tests+=(--test "$suite")
done
staging=$(mktemp -d)
{
  cargo build --locked --bins --message-format json \
    --package kestrel-supervisor --package kestrel-scripted-agent --package kestrel-client
  cargo test --locked --no-run --message-format json --package kestrel "${tests[@]}"
} | jq -r 'select(.reason == "compiler-artifact" and .executable != null)
             | "\(.target.name)\t\(.executable)"' \
  | while IFS=$'\t' read -r name executable; do
      cp "$executable" "$staging/$name"
    done
for binary in "${suites[@]}" kestrel-supervisor kestrel-scripted-agent kestrel; do
  [[ -x "$staging/$binary" ]] || { echo "Cargo produced no $binary" >&2; exit 1; }
done
tar --zstd --create --file "$archive" --directory "$staging" .
