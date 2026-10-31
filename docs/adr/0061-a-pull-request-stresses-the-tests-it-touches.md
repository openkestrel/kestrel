# A pull request stresses the tests it touches

The gate fails a test that passes only on retry ([ADR-0042](0042-a-tree-that-passed-the-gate-is-not-run-again.md)),
but a new flaky test usually passes the one run its own pull request gives it. It then fails a later,
unrelated pull request, whose author records it and re-runs, and the cost lands on someone who
cannot see the cause. We decided that the gate stresses the tests a pull request adds or changes,
and fails it when any run fails:

- **Rust:** every test in a `crates/kestrel/tests/suite/` file the pull request adds or changes,
  under `cargo nextest run --stress-count 50`, capped at about ten minutes.
- **Playwright:** every spec file it adds or changes, under `--repeat-each 20`.
- **A helper change selects nothing extra.** A change to `support/` or a shared `e2e/*.ts` reaches
  every test, which no pull request can afford to stress; the nightly Flakes run covers it.
- **It is part of the required gate**, not a check that only reports.

## Considered options

- **Rely on the nightly Flakes run.** Rejected as the only defence: it finds a flake after it
  merged, and it does not say which pull request brought it.
- **A separate check that only reports.** Rejected: a report nobody must act on goes unread.
- **Stress the whole suite on every pull request.** Rejected: the nightly run takes 45 minutes to do
  that once.

## Consequences

- A pull request that touches tests costs more CI minutes. That is the trade, made on purpose.
- A test fixed for flaking carries its own stress evidence by the same mechanism, so the manual
  stress run `docs/agents/flakes.md` asks of a fix becomes what the gate shows.
