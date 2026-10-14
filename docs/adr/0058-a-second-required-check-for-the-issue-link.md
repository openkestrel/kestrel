# A second required check for the issue link

[ADR-0027](0027-ci-gates-the-merge-and-a-queue-lands-the-batch.md) makes one always-running aggregate
job the required status, because a job skipped by a condition reports success while a workflow kept
from starting never reports at all. This adds a second one. `issue-link` is its own always-running
workflow, required beside `gate`, and it fails a pull request based on the default branch whose body
closes no issue and declares none.

The check belongs to the pull request, not to the tree. Folding it into CI would put it inside CI's
concurrency group, keyed to the branch, where an edit to a body cancels the run testing the tree; the
gate would then report green and record `gate-passed-<tree>`
([ADR-0042](0042-a-tree-that-passed-the-gate-is-not-run-again.md)) for a tree no area finished
testing, and the merge queue would reuse that pass. Re-running every area on every body edit is the
other cost, and it is not a small one. So the check is separate, and its own concurrency group means
an edit can only ever cancel its own previous run.

What it accepts is GitHub's own answer. `closingIssuesReferences` is the set GitHub will close,
computed from the body's keywords and from any issue a person linked in the sidebar, so a green check
means merging closes something. A regular expression over the body would predict that set instead of
asking for it, and the prediction is wrong on every base but the default branch.

The check is scoped to where closing works. GitHub reads a closing keyword only on a pull request
based on the default branch, and ADR-0027's ruleset is itself scoped to the default branch, so the
two scopes coincide: a pull request based on anything else passes, because it can close nothing, and
a stack's tickets are named on its top pull request.

## Considered options

- **A job in `ci.yml`, with `edited` added to its triggers.** Rejected: every body edit re-runs the
  whole of CI, images and arm64 builds included.
- **A job in `ci.yml` that runs no area on `edited`.** Rejected: the edit shares CI's concurrency
  group and cancels the run testing the tree, so the gate records a pass for a tree nothing tested —
  it breaks the very reuse ADR-0042 exists to make safe.
- **Parsing the body for a closing keyword.** Rejected: it predicts what GitHub will do rather than
  asking, and the prediction is wrong on every base but the default branch.
- **A workflow that writes the missing line itself.** Rejected: a gate that satisfies itself invents
  a reference no agent chose, and a wrong reference closes the wrong issue silently.
- **A repository setting or a ruleset rule.** Impossible: no ruleset rule and no branch-protection
  option requires a linked issue, on any plan.

## Consequences

- The ruleset carries a second required check, applied through the API beside `gate` and pinned to
  the GitHub Actions app.
- A `No issue: <reason>` line is the only way past the check, and a pull request carrying both that
  line and a closing reference fails.
- The remedy is an edit to the body, which re-triggers the check. The failure prints the line to add,
  and the check needs no write permission, so it behaves the same on a fork's pull request.
- A pull request based on any other branch carries no required check from this workflow, and none
  from ADR-0027's ruleset either, so a stack's lower pull requests are unguarded. Naming the tickets
  on the top pull request works around that gap; it does not close it.
