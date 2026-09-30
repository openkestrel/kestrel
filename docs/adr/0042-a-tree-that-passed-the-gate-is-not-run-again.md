# A tree that passed the gate is not run again

[ADR-0027](0027-ci-gates-the-merge-and-a-queue-lands-the-batch.md) re-runs the required set for every
merge queue entry, and rejected reusing a verdict keyed by the tree's content hash because the tests
are not hermetic. In the queue's first week, 16 of 38 entries were exactly the tree their pull request
had last passed, and 11 of those were full runs of about eight minutes. For an identical tree the
re-run tests no code; it only rolls the flakes again. So a queue entry whose tree passed the gate
within the last day, in a run from this repository, reports green without running any area.

The reuse is narrowed until neither of ADR-0027's objections applies:

- **Only a pass is recorded.** A failure is never reused, so a flaky failure cannot block a tree.
- **A pass lasts a day, and only a run that ran records one.** A reused pass records nothing, so a
  pass can never be carried forward past the day of its run. That bounds what the tree hash cannot see: the
  runner image and the packages `apt-get` resolves.
- **Only a run from this repository counts.** A fork's run could record a pass for any tree it liked.
- **The second roll moves off the queue.** The gate retries a failing test twice and fails anyway
  when a retry passes, so a flake reads as `FLAKY` rather than as a regression. The scheduled Flakes
  workflow runs the suite under stress on the runners the gate uses, which is where a flake the
  suite has not yet shown is found.

## Considered options

- **Keep re-running every entry.** Rejected: it spends eight minutes on four entries in ten while
  testing nothing those entries' pull requests did not.
- **Retry flaky tests and let the gate pass.** Rejected: a flake made to pass is one nobody fixes, and
  a race in the product passes with it.
- **Turborepo's remote cache.** Rejected: it keys a task on the inputs it discovers, which miss
  `openapi/`, `.kestrel/`, `images/` and the binaries the tests build with a nested `cargo build`,
  and a cache pull requests can write is the one ADR-0027 refuses.
- **`actions/cache` as the record.** Impossible: a `merge_group` run cannot read a pull request's
  cache scope. The record is an artifact, which any run in the repository can list.

## Consequences

- The record is an artifact named `gate-passed-<tree>`, uploaded by `gate` and retained for a day.
  Reading it needs `actions: read` in the `changes` job, which reports no area when one is found.
- A queue entry whose tree is not its pull request's, because `main` moved or entries are ahead of
  it in the group, still runs in full. The reuse removes the re-run only where it tested nothing new.
- ADR-0027's prerequisite stands: flakes are fixed, not tolerated. The Flakes workflow failing is a
  ticket, as [#430](https://github.com/openkestrel/kestrel/issues/430) is for the flake the queue
  caught first.
