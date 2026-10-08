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
  the supervisor is spawned; a post's `attempted_at` before the comment is posted.

## Sweeps and roles

- **Poll `Store`; nothing notifies**, apart from Organization change notices: a committed
  transaction hands what it touched to `Fanout`, the serve role's memory-only state (a running
  tool call or unit, the work summary) hands what it changed as it changes, and the change stream
  tells subscribers to refetch. The dispatch loop, link streams and Transcript streams still poll
  every 100 ms and `timer` sweeps every 500 ms. `Wake` shortens the wait after ingest within one
  process only.
- **Due times live in `Store`**, never only in memory: lease expiry, schedule `due_at`, poll due,
  held-firing reconsideration, post `due_at`. A restarted control plane finds all of them.
- **A failed pass warns and waits for the next.** Nothing in a sweep or the dispatch loop stops a
  role, because stopping the work role cuts every live supervisor off its link.
- **Work in bounded bites.** Sweeps take a fixed number of rows (32 firings, 10 poll pages) so one
  busy source cannot starve the rest.

## Errors

- `anyhow` throughout. Refuse with `declined::Reason` (wrapped in `declined::Concerning` when it is
  about a request field), and with `declined::Declined` only where a producer is not typed yet
  ([Operator boundary](operator-and-client.md#errors)). Never match a refusal's message.
- Messages are sentences a person reads: say what happened and to what, in `GLOSSARY.md`'s terms.

## Abstraction

- **No trait with one implementation.** The ports and Store repositories are concrete modules
  until a second real implementation exists ([ADR-0005](../adr/0005-six-ports-at-rung-one-are-named-boundaries.md),
  [ADR-0022](../adr/0022-store-repository-traits-and-enum-dispatch-are-deferred-to-the-postgres-rung.md)).
  `compute::Driver` is an enum, not `dyn`, for the same reason.
- **Operator wire types come from the document.** The control plane and Rust Client share the
  generated `kestrel-operator-types` leaf (ADR-0050, ADR-0051); existing endpoint migrations remain
  incremental. The link keeps independent structs on each side.
- **No branching on which harness.** The supervisor speaks ACP and nothing else; a harness-specific
  `if` is a bug ([ADR-0007](../adr/0007-acp-is-the-agent-runtime-contract.md)).
  `crates/kestrel-supervisor/tests/boundary.rs` fails the build on any agent name or control-plane
  noun (`workspace`, `transcript`, …) anywhere under the supervisor's `src/`, unit-test names
  included.

## Tests

- **The primary seam is the whole control plane.** `crates/kestrel/tests/support/` boots it in
  process against a temp SQLite file and drives it through the operator boundary and link. Assert in
  Workspaces, Sessions and Transcripts; reach `Store` and `Log` only through it.
- **Ordinary tests share one binary.** `crates/kestrel/tests/suite/` holds a module per area, so the
  harness compiles once: select with `cargo test -p kestrel --test suite <module>::`.
- **The real supervisor runs as a local process.** `LocalExec` spawns the `kestrel-supervisor` binary
  against `kestrel-scripted-agent`, so most tests need no Docker.
- **Heavier tests are `#[ignore]`d and CI runs them**, each suite its own file and binary beside
  `suite/`: `docker`, `image`, `opencode`, `development`, `control_plane`, `compose` and `cleanup`,
  each named in `.github/workflows/ci.yml`. `conformance.yml` drives a second ACP agent to prove
  nothing branches on the harness; `contention` measures load and `subscription` spends a
  person's model plan, so only a person runs them.
- **The supervisor has its own suite** in `crates/kestrel-supervisor/tests/` against a stub link.
- **Git in tests is hermetic** only if your global config is: run `cargo test` with
  `GIT_CONFIG_GLOBAL=/dev/null` when commit signing is configured.
- **CI runs the default suite with `cargo nextest`**, one process per test. Plain `cargo test` runs
  all of `suite` in one process, so a test owns its fixture and leans on nothing another left there.
- **CI is the merge gate** and a merge queue lands batches ([ADR-0027](../adr/0027-ci-gates-the-merge-and-a-queue-lands-the-batch.md)).
  Change detection is a job in the workflow, never a path filter. A queue entry whose exact tree
  already passed is not run again, and a test that passes only on retry fails the gate
  ([ADR-0042](../adr/0042-a-tree-that-passed-the-gate-is-not-run-again.md)).
