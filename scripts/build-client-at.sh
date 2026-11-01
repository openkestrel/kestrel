#!/usr/bin/env bash
# Builds the browser Client as it was at a commit and prints the directory to serve it from, for
# KESTREL_CLIENT_ROOT. Usage: scripts/build-client-at.sh [REF], REF defaulting to the merge-base
# with origin/main. Build output goes to stderr; a commit already built is reused.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
commit=$(git rev-parse --verify "${1:-$(git merge-base HEAD origin/main)}^{commit}")
copy="${TMPDIR:-/tmp}/kestrel-client-${commit:0:12}"
dist="$copy/packages/client/dist/client"

if [[ ! -f "$dist/index.html" ]]; then
  rm -rf "$copy"
  mkdir -p "$copy"
  # The copy's mise.toml would be untrusted at its new path; the tools come from this checkout's.
  git -C "$root" archive "$commit" | tar -x -C "$copy" --exclude=mise.toml
  (
    cd "$root"
    # shellcheck disable=SC2016 # $1 expands in the inner shell
    mise exec -- bash -c 'cd "$1" && bun install --frozen-lockfile && cd packages/client && bun run build' _ "$copy"
  ) >&2
fi

echo "$dist"
