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

CI is the required gate: it selects checks by changed area
(`.github/workflows/ci.yml`); the development image's expensive self-check runs separately
(`.github/workflows/development-image.yml`). Locally, run the **narrowest check** that answers the
question you have right now, and only when you have one. This machine's CPU and memory are shared with
other agents, so each local cargo run has a real cost.

- Compiles? `cargo clippy -p <crate> --all-targets -- -D warnings`, CI's lint scoped to one
  crate; it type-checks without codegen, as `cargo check` does.
- Changed `openapi/operator/*.json`? The operator document is one fragment per area, merged by
  key: put a path or schema in its area's file, and a schema several areas use in `shared.json`.
  Three consumers are generated from it at build time and none is committed (ADR-0059). Lint the
  CLI that matches on the Rust types exhaustively with
  `cargo clippy -p kestrel-client --all-targets -- -D warnings`, and typecheck the browser Client: `cd packages/client && bun install && bun run typecheck`.
  Adding or changing an Operator endpoint? See
  [the contract-change workflow](docs/architecture/operator-change-workflow.md) for its route
  inventory and focused contract checks.
- Behaves? One test target, filtered to the test:
  `cargo test -p kestrel --test suite <module>::<name>`. The ordinary control-plane tests are one
  binary, `crates/kestrel/tests/suite/`, a module per area (`work`, `operator`, …); a heavy
  suite (`docker`, `image`, `compose`, …) is its own file in `crates/kestrel/tests/` and binary
  (`--test docker`). The tests build the supervisor and scripted agent themselves, so a
  whole-package or workspace run is many builds, not one.
- Batch edits before checking; one check after a coherent change, never one per line.
- Before pushing, `cargo fmt --all`. Leave the workspace suite and the `#[ignore]`d
  suites to CI. CI runs only on a pull request, so finish by pushing the branch and opening one;
  then read what failed with `gh pr checks` and `gh run view --log-failed`. A branch that conflicts
  with `main` gets no `pull_request` run at all, so `gh pr checks` shows no `gate` row; `gate`
  appearing is the sign the run started, and merging `main` and pushing until it does is the way
  there.
  For completed failed-job logs while the run is still active, or logs rejected for terminal escape
  sequences, see [CI diagnosis](docs/agents/ci-diagnosis.md).
- Documentation, skill and agent-guidance changes need no local check.

Where a skill asks for a full-suite run ("the full test suite", "the project's automated checks",
or a passing run as PR evidence), in this repo that means pushing and reading CI; the narrow local
checks above still answer a single behaviour or type question. For implementation reviews, capture
the branch's starting commit before editing and pass it as the fixed point to code-review, unless
the user supplies another baseline. The test seams documented in
[conventions](docs/architecture/conventions.md#tests) are pre-agreed; confirm a new seam before
writing tests against it.

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

Kestrel is an unreleased early prototype with no users. Do not preserve backward compatibility or
write data migrations for existing installations; change the original schema and migrations
directly until the project is released.

### Comments

Write a comment only where a reader would otherwise get it wrong, in one sentence. Older code
carries more than that and is not the model. Review holds the diff to `CODING_STANDARDS.md`.
