#!/usr/bin/env bash
# Drives retry.sh against a command that fails the way a registry does, so which failures are
# retried and which are raised at once is checked without a registry to fail on purpose.
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "${work}"' EXIT

# The backoff is recorded rather than slept: a shim earlier on PATH takes retry.sh's `sleep`, so the
# test does not wait out the schedule it is checking.
export SLEEP_LOG="${work}/slept"
mkdir -p "${work}/bin"
cat >"${work}/bin/sleep" <<'SCRIPT'
#!/usr/bin/env bash
echo "$1" >>"${SLEEP_LOG}"
SCRIPT
chmod +x "${work}/bin/sleep"
export PATH="${work}/bin:${PATH}"

fail() {
  echo "$1" >&2
  exit 1
}

# A step that fails as told, counting its own runs.
cat >"${work}/registry" <<'SCRIPT'
#!/usr/bin/env bash
set -uo pipefail
mode="$1" runs="$2"
count=$(cat "${runs}" 2>/dev/null || echo 0)
count=$((count + 1))
echo "${count}" >"${runs}"
case "${mode}" in
ok) ;;
transient)
  [[ ${count} -ge 3 ]] || { echo "unknown blob" >&2; exit 1; }
  ;;
always)
  echo "unknown blob" >&2
  exit 1
  ;;
denied)
  echo "denied: requested access to the resource is denied" >&2
  exit 17
  ;;
invalid)
  echo "manifest invalid" >&2
  exit 1
  ;;
esac
echo "pushed"
SCRIPT
chmod +x "${work}/registry"

status=0
run() {
  : >"${work}/out"
  status=0
  "${root}/retry.sh" "$@" >>"${work}/out" 2>&1 || status=$?
}

runs() { cat "${work}/runs" 2>/dev/null || echo 0; }
slept() { [[ -f "${work}/slept" ]] && tr '\n' ' ' <"${work}/slept"; }
reset() { rm -f "${work}/runs" "${work}/slept"; }

reset
run "${work}/registry" ok "${work}/runs"
[[ ${status} -eq 0 ]] || fail "a step that succeeded exited ${status}"
[[ "$(runs)" == 1 ]] || fail "a step that succeeded ran $(runs) times"
[[ -z "$(slept)" ]] || fail "a step that succeeded was backed off for $(slept)"

reset
run "${work}/registry" transient "${work}/runs"
[[ ${status} -eq 0 ]] || fail "a transient failure was not retried to success: exit ${status}"
[[ "$(runs)" == 3 ]] || fail "a transient failure ran $(runs) times, expected 3"
[[ "$(slept)" == "5 10 " ]] || fail "the backoff was '$(slept)', expected '5 10'"

reset
run "${work}/registry" denied "${work}/runs"
[[ ${status} -eq 17 ]] || fail "a refusal exited ${status}, expected the command's 17"
[[ "$(runs)" == 1 ]] || fail "a refusal ran $(runs) times, expected 1"
[[ -z "$(slept)" ]] || fail "a refusal spent the budget: backed off for $(slept)"
grep -q 'denied' "${work}/out" || fail "a refusal did not say why it was raised at once"

reset
run "${work}/registry" invalid "${work}/runs"
[[ ${status} -eq 1 ]] || fail "a rejected manifest exited ${status}, expected 1"
[[ "$(runs)" == 1 ]] || fail "a rejected manifest ran $(runs) times, expected 1"

reset
run "${work}/registry" always "${work}/runs"
[[ ${status} -eq 1 ]] || fail "a step that never clears exited ${status}, expected 1"
[[ "$(runs)" == 3 ]] || fail "a step that never clears ran $(runs) times, expected the bound of 3"
[[ "$(slept)" == "5 10 " ]] || fail "the backoff was '$(slept)', expected '5 10'"
grep -q 'failed 3 times' "${work}/out" || fail "a step that never clears did not say so"

: >"${work}/out"
status=0
"${root}/retry.sh" >>"${work}/out" 2>&1 || status=$?
[[ ${status} -eq 2 ]] || fail "no command exited ${status}, expected the usage error 2"

echo "retry.sh retries a transient failure and raises a refusal at once."
