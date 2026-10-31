## Agent skills

### Issue tracker

Issues are tracked in GitHub (`openkestrel/kestrel`). Fetch one with
`gh issue view <number> --repo openkestrel/kestrel --json title,body,labels,comments`; the rest is
in `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical triage labels (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix) and a five-rung difficulty vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (root `GLOSSARY.md` + `docs/adr/`). See `docs/agents/domain.md`.

### Architecture

`docs/architecture/README.md` maps processes, crates, ports, trust boundaries, and the ADRs the code
has not caught up to. Read the map, then the page in `docs/architecture/` for what you are changing:

- queueing, claiming or ending work: `sessions.md`
- what the supervisor and control plane say to each other: `link.md`
- Events, matching, follow-ups, posts: `triggers.md`
- the schema or a query: `data-model.md`
- any operator endpoint, its errors or a declaration: `operator-boundary.md`
- the Operator, readiness, sign-ins, the harness catalogue: `operator-setup.md`
- a Session or Workspace read, a stream, live work: `operator-reads.md`
- a `kestrel` command or the browser Client: `client.md`
- transactions, errors, tests, in every change: `conventions.md`

## Code

### Checks

For a fresh checkout, helper preparation, target isolation or Git-fixture setup, read
[CONTRIBUTING.md](CONTRIBUTING.md). Run commands through `mise exec --` unless mise is activated
in the shell.

CI is the required gate: it selects checks by changed area
(`.github/workflows/ci.yml`); the development image's expensive self-check runs separately
(`.github/workflows/development-image.yml`). Locally, run the **narrowest check** that answers the
question you have right now, and only when you have one. This machine's CPU and memory are shared with
other agents, so each local cargo run has a real cost.

- Compiles? `cargo clippy -p <crate> --all-targets -- -D warnings`, CI's lint scoped to one
  crate; it type-checks without codegen, as `cargo check` does.
- Changed `openapi/*.json`? Three consumers are generated from it. Regenerate the Rust types with
  `mise run generate-check`, the task CI's own step runs; it writes the files and exits 1 until
  what it wrote is committed. Lint the CLI that matches on them exhaustively with
  `cargo clippy -p kestrel-client --all-targets -- -D warnings`, and typecheck the browser Client: `cd packages/client && bun install && bun run typecheck`.
  Adding or changing an Operator endpoint? See
  [the contract-change workflow](docs/architecture/operator-change-workflow.md) for its route
  inventory and focused contract checks.
- Behaves? One test target, filtered to the test:
  `cargo test -p kestrel --test suite <module>::<name>`. The ordinary control-plane tests are one
  binary, `crates/kestrel/tests/suite/`, a module per area (`work`, `operator`, …); a heavy
  suite (`docker`, `image`, `compose`, …) is its own file in `crates/kestrel/tests/` and binary
  (`--test docker`). Cargo prepares the supervisor, scripted agent and Client helpers once
  per invocation.
- Batch edits before checking; one check after a coherent change, never one per line.
- Before pushing, `cargo fmt --all`. Leave the workspace suite and the `#[ignore]`d
  suites to CI. CI runs only on a pull request, so finish by pushing the branch and opening one;
  then read what failed with `gh pr checks` and `gh run view --log-failed`. A branch that conflicts
  with `main` gets no `pull_request` run at all, so `gh pr checks` shows no `gate` row; `gate`
  appearing is the sign the run started, and merging `main` and pushing until it does is the way
  there.
  For completed failed-job logs while the run is still active, or logs rejected for terminal escape
  sequences, see [CI diagnosis](docs/agents/ci-diagnosis.md).
- **Flaky**, or failing where your change cannot reach? Record it before any re-run:
  [flakes](docs/agents/flakes.md).
- Documentation, skill and agent-guidance changes need no local check.

Where a skill asks for a full-suite run ("the full test suite", "the project's automated checks",
or a passing run as PR evidence), in this repo that means pushing and reading CI; the narrow local
checks above still answer a single behaviour or type question. For implementation reviews, capture
the branch's starting commit before editing and pass it as the fixed point to code-review, unless
the user supplies another baseline. The test seams documented in
[conventions](docs/architecture/conventions.md#tests) are pre-agreed; confirm a new seam before
writing tests against it.

### File size

`.githooks/pre-commit` refuses a staged file over 5,000 tokens (bytes / 4) and says how to split
it; mise installs it (`git config core.hooksPath .githooks`). Lockfiles, generated code and
captured output are exempt through `linguist-generated` in `.gitattributes`; files written before
the budget carry a `token-budget-exempt=<issue>` line there until that issue splits them.

### Pull requests

A pull request that closes an issue names it in the body, and only one based on `main` can: GitHub
ignores a closing keyword on any other base. The `issue-link` check fails a `main`-based pull request
that names none, so name the issue the work came from — it is in the brief that dispatched it — or,
when it genuinely closes none, write a `No issue: <reason>` line. A stack names its tickets on the
top pull request; the rest is in `docs/agents/issue-tracker.md`.

### Visual evidence

A pull request that changes what the browser Client shows carries screenshots, and a recording when
something moves, in its Evidence section. [Visual evidence](docs/agents/visual-evidence.md) says
what to show, how to capture it with `playwright-cli` from a paused e2e spec, and how to upload it
with `gh pr edit --attach`.

### Debug info

Workspace code builds with line-tables-only debuginfo and dependencies with none, so builds are
smaller and faster but a debugger sees file and line numbers, not variables or types. Restore
full debug info with `--profile full-debug`:

    cargo test --profile full-debug -p kestrel --test suite <module>::<name>
    cargo build --profile full-debug -p kestrel

It builds into `target/full-debug`, so alternating with the default recompiles. `cargo nextest`
takes the flag as `--cargo-profile full-debug`.

### Compatibility

Kestrel is unreleased but dogfooded, so its schema moves forward. A migration on `main` is never
edited, renumbered or deleted; a schema change is a new migration, named by UTC timestamp
(`20261013154500_<name>.sql`) so two branches never claim the same number. That migration may be
backwards-incompatible and may drop data: the previous schema is never a reason to keep a shape you
would not choose now, and there are no compatibility shims or backfills for an earlier build's data.

### Comments

Write a comment only where a reader would otherwise get it wrong, in one sentence. Older code
carries more than that and is not the model. Review holds the diff to `CODING_STANDARDS.md`.
