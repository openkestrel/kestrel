#!/usr/bin/env bash
# Retries an idempotent registry step — a sign-in or a push — three times with an exponential
# backoff, and raises a refusal a retry cannot fix on the attempt that produced it
# (docs/agents/flakes.md).
#
#   retry.sh COMMAND [ARGUMENT...]
#
# The command runs again only when its output names no refusal below, so `unknown blob` — the
# registry refusing a blob the manifest it is writing references, after every blob reported
# uploaded — is retried, and `denied` is not.
set -euo pipefail

readonly attempts=3
readonly delay=5

if [[ $# -eq 0 ]]; then
  echo "usage: retry.sh COMMAND [ARGUMENT...]" >&2
  exit 2
fi

# What a registry says when no number of attempts will change the answer: `denied` carries the 403s
# of a token without `packages: write`, a repository the actor cannot reach, an over-quota one.
refusals=(
  'denied'
  'forbidden'
  'unauthorized'
  'authentication required'
  'insufficient_scope'
  'no basic auth credentials'
  'invalid username or password'
  'manifest invalid'
  'invalid manifest'
  'invalid reference format'
  'name unknown'
  'repository does not exist'
  'quota'
)

refusal_named_by() {
  local reason
  for reason in "${refusals[@]}"; do
    if grep -qiF -- "${reason}" "$1"; then
      echo "${reason}"
      return 0
    fi
  done
  return 1
}

log=$(mktemp)
trap 'rm -f "${log}"' EXIT

for ((attempt = 1; attempt <= attempts; attempt++)); do
  status=0
  "$@" 2>&1 | tee "${log}" || status=$?
  if [[ ${status} -eq 0 ]]; then
    exit 0
  fi

  if reason=$(refusal_named_by "${log}"); then
    echo "::error::$1 was refused with '${reason}', which a retry cannot change" >&2
    exit "${status}"
  fi

  if [[ ${attempt} -eq ${attempts} ]]; then
    echo "::error::$1 failed ${attempts} times" >&2
    exit "${status}"
  fi

  backoff=$((delay * 2 ** (attempt - 1)))
  echo "::warning::$1 failed on attempt ${attempt} of ${attempts}, exit ${status}; retrying in ${backoff}s" >&2
  sleep "${backoff}"
done
