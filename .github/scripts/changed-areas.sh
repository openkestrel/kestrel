#!/usr/bin/env bash
# Classifies the paths a change touched into the areas CI runs. Reads the changed paths from
# stdin, one per line, and writes a `true`/`false` per area to stdout and to GITHUB_OUTPUT.
set -euo pipefail
# Globs are matched, never expanded: without this, a glob word in a loop is a pathname the
# shell tries to expand against the checkout.
set -f

readonly areas="rust images arm64 compose client client-image release docs"

# Each area declares the paths that make it run, in one table: `inputs` and `matches` walk the
# same list, so neither can name an area the other misses. `docs` declares what no heavy area
# consumes, so a documentation-only change is recognised and the heavy areas stay off. A
# positional list rather than an associative one, because the shell CI and laptops run is bash
# 3.2, which has no `declare -A`.
area_inputs() {
  case "$1" in
  rust) echo 0 ;;
  images) echo 1 ;;
  arm64) echo 2 ;;
  compose) echo 3 ;;
  client) echo 4 ;;
  client-image) echo 5 ;;
  release) echo 6 ;;
  docs) echo 7 ;;
  esac
}
readonly inputs=(
  'crates/** Cargo.toml Cargo.lock rust-toolchain.toml mise.toml .config/** openapi/** .kestrel/** .github/**'
  'images/kestrel/** images/kestrel-env/** images/kestrel-dev/** crates/** Cargo.toml Cargo.lock rust-toolchain.toml .dockerignore openapi/** .kestrel/** .github/**'
  'images/kestrel-env/** images/kestrel-dev/** .dockerignore .github/**'
  'compose.yaml crates/** Cargo.toml Cargo.lock rust-toolchain.toml images/** .dockerignore openapi/** .kestrel/** .github/** packages/** package.json bun.lock'
  'packages/** package.json bun.lock mise.toml openapi/** crates/** Cargo.toml Cargo.lock rust-toolchain.toml images/kestrel-client/** .github/**'
  'images/kestrel-client/** .dockerignore packages/** package.json bun.lock openapi/** .github/**'
  'scripts/** Cargo.toml Cargo.lock crates/**/Cargo.toml CHANGELOG.md package.json packages/**/package.json packages/client/public/version.json mise.toml .github/**'
  'docs/** *.md LICENSE .gitignore .agents/** .claude/** skills-lock.json'
)

matches() {
  local path="$1" area="$2" glob
  # shellcheck disable=SC2086 # the glob list is deliberately word-split
  for glob in ${inputs[$(area_inputs "$area")]}; do
    # shellcheck disable=SC2053 # the right-hand side is a glob on purpose
    [[ "$path" == $glob ]] && return 0
  done
  return 1
}

paths=()
while IFS= read -r path; do
  [[ -n "$path" ]] && paths+=("$path")
done

touched=""
unrecognised=false
# A bash-3 array with nothing in it is unbound under `set -u`, so an empty change is its own
# case rather than an empty iteration.
if [[ ${#paths[@]} -eq 0 ]]; then
  unrecognised=true
else
  for path in "${paths[@]}"; do
    declared=false
    for area in $areas; do
      if matches "$path" "$area"; then
        touched="$touched $area"
        declared=true
      fi
    done
    if ! $declared; then
      unrecognised=true
    fi
  done
fi

# A path no area declares, or no readable change at all, is a reason to run everything: a
# required check that silently does not run is worse than a slow one.
for area in rust images arm64 compose client client-image release; do
  changed=$unrecognised
  [[ " $touched " == *" $area "* ]] && changed=true

  echo "$area=$changed"
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    echo "$area=$changed" >>"$GITHUB_OUTPUT"
  fi
done
