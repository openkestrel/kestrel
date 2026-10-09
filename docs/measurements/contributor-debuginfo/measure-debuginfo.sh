#!/usr/bin/env bash
# Measures the workspace's debuginfo profiles on the contributor baseline's workloads (#592).
# Writes results.txt and flags-*.txt under the work directory it is given (default: a fresh temp
# directory), and leaves the checkout as it found it.
set -euo pipefail

root=$(cd "$(dirname "$0")/../../.." && pwd)
work=${1:-$(mktemp -d "${TMPDIR:-/tmp}/kestrel-debuginfo.XXXXXX")}
logs=$work/logs
results=$work/results.txt
focused=work::a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance
prefix=(mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1)

mkdir -p "$logs"
: >"$results"

cd "$root"
lib=crates/kestrel/src/lib.rs
cp "$lib" "$work/lib.rs.bak"
trap 'cp "$work/lib.rs.bak" "$lib"' EXIT

# Each crate's rustc debuginfo and strip flags, one line each, from a verbose build log.
extract_flags() {
  awk '/Running/ && /rustc/ {
    crate = "?"; debuginfo = "none"; strip = "none";
    for (i = 1; i <= NF; i++) {
      if ($i == "--crate-name") crate = $(i + 1);
      if ($i == "-C" && $(i + 1) ~ /^debuginfo=/) debuginfo = $(i + 1);
      if ($i == "-C" && $(i + 1) ~ /^strip=/) strip = $(i + 1);
    }
    print crate, debuginfo, strip;
  }' "$1" | sort -u >"$2"
}

# timed <label> <log> <command...>: records the wall time, Cargo's own summary and, for tests,
# the harness line, then appends the artifacts' sizes to the results.
timed() {
  local label=$1 log=$2
  shift 2
  printf '\n=== %s ===\n' "$label" >>"$results"
  /usr/bin/time -p "$@" >"$log" 2>&1
  grep -E '^real|Finished|test result' "$log" >>"$results"
}

record_sizes() { # record_sizes <label> <target> <profile subdir>
  local label=$1 target=$2 subdir=$3
  printf '\n--- %s\n' "$label" >>"$results"
  printf 'target %s KiB\n' "$(du -sk "$target" | cut -f1)" >>"$results"
  ls -l "$target/$subdir/kestrel-control-plane" \
    "$target/$subdir/kestrel-supervisor" \
    "$target/$subdir/kestrel-scripted-agent" 2>/dev/null |
    awk '{print $NF, $5}' >>"$results" || true
  ls -l "$target/$subdir/deps"/suite-* 2>/dev/null |
    grep -vE '\.(d|o)$' |
    awk '{print $NF, $5}' >>"$results" || true
}

measure() { # measure <name> <profile subdir> [profile name]
  local name=$1 subdir=$2 profile=${3:-}
  local target=$work/$name
  local build=(cargo build --locked -p kestrel -v)
  local test_compile=(cargo test --locked -p kestrel --test suite --no-run -v)
  local test=(cargo test --locked -p kestrel --test suite)
  local noop=(cargo build --locked -p kestrel)
  if [[ -n $profile ]]; then
    build+=(--profile "$profile")
    test_compile+=(--profile "$profile")
    test+=(--profile "$profile")
    noop+=(--profile "$profile")
  fi
  mkdir -p "$target"

  local log=$logs/build-$name.txt
  timed "clean control-plane build ($name)" "$log" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${build[@]}"
  extract_flags "$log" "$work/flags-dev-$name.txt"
  record_sizes "after the clean build ($name)" "$target" "$subdir"

  log=$logs/test-compile-$name.txt
  timed "one integration-test binary compilation ($name)" "$log" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${test_compile[@]}"
  extract_flags "$log" "$work/flags-test-$name.txt"
  record_sizes "after the test-binary compile ($name)" "$target" "$subdir"

  timed "first focused test ($name)" "$logs/first-test-$name.txt" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${test[@]}" "$focused" -- --exact
  record_sizes "after the first focused test ($name)" "$target" "$subdir"

  timed "warm focused test ($name)" "$logs/warm-test-$name.txt" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${test[@]}" "$focused" -- --exact

  timed "no-op control-plane build ($name)" "$logs/noop-$name.txt" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${noop[@]}"

  printf '\n' >>"$lib"
  timed "incremental control-plane build ($name)" "$logs/incremental-$name.txt" \
    "${prefix[@]}" env CARGO_TARGET_DIR="$target" "${noop[@]}"
  cp "$work/lib.rs.bak" "$lib"
  record_sizes "after all workloads ($name)" "$target" "$subdir"

  rm -rf "$target"
}

measure reduced debug
measure full-debug full-debug full-debug

echo "results in $work"
