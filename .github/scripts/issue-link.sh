#!/usr/bin/env bash
# Fails a pull request that closes no issue and declares none. The issues it closes are read from
# GitHub rather than parsed out of the body, so what passes is what merging will actually close.
set -euo pipefail

readonly pr="${1:?usage: issue-link.sh <pull-request-number>}"
readonly default_branch="${DEFAULT_BRANCH:-main}"

fail() {
  local message="$1"
  echo "$message" >&2
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf '%s\n' "$message" >>"$GITHUB_STEP_SUMMARY"
  fi
  exit 1
}

json=$(gh pr view "$pr" --json baseRefName,body,closingIssuesReferences)

base=$(jq -r '.baseRefName' <<<"$json")
closing=$(jq '.closingIssuesReferences | length' <<<"$json")

# GitHub reads a closing keyword only on a pull request whose base is the default branch; on any
# other base there is no link and merging closes nothing, so a stacked ticket pull request passes
# here and the stack's top pull request names its tickets.
if [[ "$base" != "$default_branch" ]]; then
  echo "Based on \`${base}\`, not \`${default_branch}\`: this pull request closes nothing, and the stack's top pull request names its tickets."
  exit 0
fi

# An opt-out is a line of its own with a reason after it, not a mention anywhere in the prose.
optout=$(jq -r '.body // ""' <<<"$json" | grep -ciE '^[[:space:]]*no issue:[[:space:]]*[^[:space:]]' || true)

if ((closing > 0)) && ((optout > 0)); then
  fail "This pull request both closes ${closing} issue(s) and declares \`No issue:\`. Say one thing or the other."
fi

if ((closing > 0)); then
  echo "Closes ${closing} issue(s)."
  exit 0
fi

if ((optout > 0)); then
  echo "Declares no issue."
  exit 0
fi

fail "This pull request closes no issue and names none.

Add the issue it closes to the body; editing the body re-runs this check:

    Closes #<issue>

If it closes no issue, say so on a line of its own instead:

    No issue: <one-line reason>

An issue that should close when a stack lands is named on the stack's top pull request."
