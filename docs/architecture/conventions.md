# Conventions

Patterns the code relies on that no single file states. Comment style is in `AGENTS.md`.

## Transactions

- **Every state change is one transaction with its Transcript entry** ([ADR-0004](../adr/0004-store-and-log-are-one-transactional-domain.md)).
  Take `store.begin()`, reach repositories through `tx.workspaces()`, `tx.triggers()` and so on,
  append with `tx.log()`, and commit once. A retry after a failed commit must be harmless.
- **`Store::begin` takes the write lock up front** (`BEGIN IMMEDIATE`). A deferred transaction that
  reads then writes is refused by SQLite under contention rather than queued. Use `Store::read`
  only for work that never writes.
- **Contention is not failure.** `store::busy(&error)` recognises `SQLITE_BUSY`/`LOCKED`. Boundaries
  answer it with `503` and `Retry-After`; the work role retries it (`until_not_busy`).
- **Guard state changes in SQL.** Transitions are written as `UPDATE … WHERE state = ?` and act on
  `rows_affected`, so a replay or a race does nothing instead of doing it twice (`prompt_turn`,
  `answer_turn`, `end_session`).
- **Commit before the side effect that depends on it.** The `start` instruction is committed before
  the supervisor is spawned; a delivery's `attempted_at` before the comment is posted.

## Sweeps and roles

- **Poll `Store`; nothing notifies**, apart from Organization change notices: a committed
  transaction hands what it touched to `Fanout`, and the change stream tells subscribers to
  refetch. The dispatch loop, link streams and Transcript streams still poll every 100 ms and
  `timer` sweeps every 500 ms. `Wake` shortens the wait after ingest within one process only.
- **Due times live in `Store`**, never only in memory: lease expiry, schedule `due_at`, poll due,
  held-firing reconsideration, delivery `due_at`. A restarted control plane finds all of them.
- **A failed pass warns and waits for the next.** Nothing in a sweep or the dispatch loop stops a
  role, because stopping the work role cuts every live supervisor off its link.
- **Work in bounded bites.** Sweeps take a fixed number of rows (32 firings, 10 poll pages) so one
  busy source cannot starve the rest.

## Errors

- `anyhow` throughout. Refuse with `declined::Declined` when a boundary should answer something
  other than 503 ([Operator boundary](operator-and-client.md#errors)).
- Messages are sentences a person reads: say what happened and to what, in `GLOSSARY.md`'s terms.

## Abstraction

- **No trait with one implementation.** The ports and Store repositories are concrete modules
  until a second real implementation exists ([ADR-0005](../adr/0005-six-ports-at-rung-one-are-named-boundaries.md),
  [ADR-0022](../adr/0022-store-repository-traits-and-enum-dispatch-are-deferred-to-the-postgres-rung.md)).
  `compute::Driver` is an enum, not `dyn`, for the same reason.
- **No shared types across a process boundary.** The supervisor, control plane and Client each
  define their own structs for the OpenAPI documents they speak.
- **No branching on which harness.** The supervisor speaks ACP and nothing else; a harness-specific
  `if` is a bug ([ADR-0007](../adr/0007-acp-is-the-agent-runtime-contract.md)).
  `crates/kestrel-supervisor/tests/boundary.rs` fails the build on any agent name or control-plane
  noun (`workspace`, `transcript`, …) anywhere under the supervisor's `src/`, unit-test names
  included.

## Tests

- **The primary seam is the whole control plane.** `crates/kestrel/tests/support/` boots it in
  process against a temp SQLite file and drives it through the operator boundary and link. Assert in
  Workspaces, Sessions and Transcripts; reach `Store` and `Log` only through it.
- **The real supervisor runs as a local process.** `LocalExec` spawns the `kestrel-supervisor` binary
  against `kestrel-scripted-agent`, so most tests need no Docker.
- **Heavier tests are `#[ignore]`d and CI runs them**: `docker`, `image`, `opencode`, `development`
  and `control_plane`, each named in `.github/workflows/ci.yml`. `conformance.yml` drives a second
  ACP agent to prove nothing branches on the harness.
- **The supervisor has its own suite** in `crates/kestrel-supervisor/tests/` against a stub link.
- **Git in tests is hermetic** only if your global config is: run `cargo test` with
  `GIT_CONFIG_GLOBAL=/dev/null` when commit signing is configured.
- **CI runs the default suite with `cargo nextest`**, one process per test, so a test cannot lean on
  state another left in the process. `cargo test` still runs it locally.
- **CI is the merge gate** and a merge queue lands batches ([ADR-0027](../adr/0027-ci-gates-the-merge-and-a-queue-lands-the-batch.md)).
  Change detection is a job in the workflow, never a path filter. A queue entry whose exact tree
  already passed is not run again, and a test that passes only on retry fails the gate
  ([ADR-0042](../adr/0042-a-tree-that-passed-the-gate-is-not-run-again.md)).
