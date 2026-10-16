# Flakes

A **flake** is a failure that changes verdict on the same tree. Flakes are fixed, never retried away
([ADR-0027](../adr/0027-ci-gates-the-merge-and-a-queue-lands-the-batch.md),
[ADR-0042](../adr/0042-a-tree-that-passed-the-gate-is-not-run-again.md)): the gate fails a test that
passes only on retry, and a flake in the merge queue drops the whole batch. Unrelated to your change
is still yours to record.

- A **test flake** is a test whose result varies: nextest reports it `FLAKY` (`TRY 2 PASS`), or it
  failed on a run and passed on a re-run of the same commit. Its fix is in code.
- An **infra flake** is a failed step that is not a test: a registry push, a login, a package fetch.
  A bounded retry on that step is a legitimate fix, and `.github/scripts/retry.sh` is what a
  registry step retries through: it re-runs a transient failure a bounded number of times with a
  backoff, and raises a refusal — a credential, a permission, a rejected manifest — at once.

## When your pull request goes red on one

1. **Record it before you re-run.** Search for its issue:
   `gh issue list --repo openkestrel/kestrel --label flaky --state all --search "<test name>"`.
   - Open: comment the run link, the commit and the failure excerpt.
   - Closed: reopen it with that comment; the fix did not hold.
   - None: file one titled `Flaky: <test name>`, labelled `flaky`, `bug`, `ready-for-agent` and a
     difficulty, with the same three facts and the suite it runs in.
2. **Fix it now if it is cheap**, in its own pull request based on `main` that closes the issue. Cheap
   means the cause is visible from the failure output and the test's own code, and the fix stays in
   that test: a fixed sleep or deadline replaced by waiting on the condition, a shared resource made
   the test's own, an ordering it assumed loosened. A fix that touches product code, a `support/`
   helper other tests use, or needs reproducing under stress first stays an issue.
3. **Re-run the failed jobs** with `gh run rerun <run-id> --failed`.

## Closing a flake issue

A fix pull request carries a stress run as evidence: the test failing under stress before the fix
and passing after, on the runners the gate uses. Dispatch the Flakes workflow filtered to the test
(`gh workflow run flakes.yml`, once [#750](https://github.com/openkestrel/kestrel/issues/750) adds the
filter), or run it locally when it reproduces there:
`cargo nextest run -p kestrel --test suite --stress-count 200 <module>::<name>`, or
`bun run e2e --repeat-each 50 <spec>` in `packages/client` for a Playwright spec. One passing run is not evidence.
