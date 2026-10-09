#!/usr/bin/env bash
set -euo pipefail
shopt -s nullglob

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
target=${CARGO_TARGET_DIR:-"$root/target"}
mkdir -p "$target"
target=$(cd "$target" && pwd -P)
profile=${HELPER_TEST_PROFILE:-dev}
mode=${1:-all}
case "$mode" in all|cargo|nextest|artifact|provenance|inplace) ;; *) echo "Unknown check: $mode" >&2; exit 1 ;; esac
directory=$target/$profile
if [[ "$profile" == dev ]]; then directory=$target/debug; fi
output=$(mktemp)
backup=
restore_in_place=false
cleanup() {
  if [[ -n "$backup" && -f "$backup" ]]; then
    if "$restore_in_place"; then
      cat "$backup" > "$directory/kestrel-supervisor"
      touch -r "$backup" "$directory/kestrel-supervisor"
      rm "$backup"
    else
      mv "$backup" "$directory/kestrel-supervisor"
    fi
  fi
  rm -f "$output"
}
trap cleanup EXIT
export GIT_CONFIG_GLOBAL=/dev/null
unset NEXTEST_RUN_ID

focused() {
  cargo test -p kestrel --test suite --profile "$profile" --target-dir "$target" \
    work::a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance -- --nocapture \
    > "$output" 2>&1
}

passing() {
  "$@" || { cat "$output"; return 1; }
}

prepared_once() {
  count=$(grep -c 'Preparing executable test helpers in ' "$output" || true)
  if [[ "$count" != 1 ]]; then
    cat "$output"
    echo "Expected one helper preparation, found $count" >&2
    return 1
  fi
  local stamps=("$directory"/.kestrel-helpers-*.stamp)
  if [[ "${#stamps[@]}" != 1 ]]; then
    echo "Expected one helper preparation stamp, found ${#stamps[@]}" >&2
    return 1
  fi
}

if [[ "$mode" == all || "$mode" == cargo ]]; then
  for helper in kestrel-supervisor kestrel-scripted-agent kestrel; do
    rm -f "$directory/$helper"
  done
  passing focused
  prepared_once
  for helper in kestrel-supervisor kestrel-scripted-agent kestrel; do
    [[ -x "$directory/$helper" ]] || { echo "Missing helper: $directory/$helper" >&2; exit 1; }
  done
  rm "$directory/kestrel-supervisor"
  unused="$target/.helper-preparation-unused-$$"
  CARGO_TARGET_DIR="$unused" passing focused
  prepared_once
  [[ -x "$directory/kestrel-supervisor" ]]
  [[ ! -d "$unused" ]]
fi

if [[ "$mode" == all || "$mode" == inplace ]]; then
  unset NEXTEST_RUN_ID
  passing focused
  prepared_once
  backup="$directory/.kestrel-supervisor-backup-$$"
  cp -p "$directory/kestrel-supervisor" "$backup"
  restore_in_place=true
  printf '#!/bin/sh\nexec "%s" "$@"\n' "$backup" > "$directory/kestrel-supervisor"
  passing focused
  prepared_once
  if ! cmp -s "$backup" "$directory/kestrel-supervisor"; then
    echo "A new invocation passed against a helper overwritten in place" >&2
    exit 1
  fi
  cleanup
  backup=
  restore_in_place=false
fi

nextest_focused() {
  cargo nextest run -p kestrel --test suite --cargo-profile "$profile" --target-dir "$target" \
    --success-output immediate --test-threads 2 \
    -E 'test(=work::a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance) | test(=cli::a_role_boots_on_an_empty_data_directory_and_makes_its_database)' \
    > "$output" 2>&1 || { cat "$output"; return 1; }
}

if [[ "$mode" == all || "$mode" == nextest ]]; then
  passing nextest_focused
  prepared_once
  rm "$directory/kestrel"
  passing nextest_focused
  prepared_once
  [[ -x "$directory/kestrel" ]]
  touch "$root/crates/kestrel-scripted-agent/src/main.rs"
  passing nextest_focused
  prepared_once
  grep -F 'Compiling kestrel-scripted-agent' "$output"
fi

if [[ "$mode" == all || "$mode" == artifact ]]; then
  export NEXTEST_RUN_ID="helper-preparation-$$-$RANDOM"
  passing focused
  prepared_once
  backup="$directory/.kestrel-supervisor-backup-$$"
  mv "$directory/kestrel-supervisor" "$backup"
  printf '#!/bin/sh\nexec "%s" "$@"\n' "$backup" > "$directory/kestrel-supervisor"
  chmod +x "$directory/kestrel-supervisor"
  if focused; then
    echo "A replaced helper passed against an earlier preparation" >&2
    exit 1
  fi
  grep -F "executable helper is stale: $directory/kestrel-supervisor" "$output"
  rm "$directory/kestrel-supervisor"
  if focused; then
    echo "A missing helper passed against an earlier preparation" >&2
    exit 1
  fi
  grep -F "executable helper is missing: $directory/kestrel-supervisor" "$output"
  mv "$backup" "$directory/kestrel-supervisor"
  backup=
fi

if [[ "$mode" == all || "$mode" == provenance ]]; then
  unset NEXTEST_RUN_ID
  passing focused
  prepared_once
  backup="$directory/.kestrel-supervisor-backup-$$"
  mv "$directory/kestrel-supervisor" "$backup"
  printf '#!/bin/sh\nexec "%s" "$@"\n' "$backup" > "$directory/kestrel-supervisor"
  chmod +x "$directory/kestrel-supervisor"
  passing focused
  prepared_once
  if ! cmp -s "$backup" "$directory/kestrel-supervisor"; then
    echo "A new invocation passed against a hand-placed helper" >&2
    exit 1
  fi
  mv "$backup" "$directory/kestrel-supervisor"
  backup=
fi
