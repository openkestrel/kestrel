## Agent skills

### Issue tracker

Issues are tracked in GitHub (`openkestrel/kestrel`). Fetch one with
`gh issue view <number> --comments`; the rest is in `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical triage labels (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix) and a five-rung difficulty vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (root `GLOSSARY.md` + `docs/adr/`). See `docs/agents/domain.md`.

### Architecture

`docs/architecture/README.md` maps processes, crates, ports, trust boundaries, and the ADRs the code
has not caught up to, and routes to one page per area: Sessions, the link, Triggers, the data
model, the operator boundary and Client, and conventions. Read the map, then the page for the area
you are changing.

## Code

### Checks

CI is the full gate: it runs fmt, clippy, the build and every test on every pull request
(`.github/workflows/ci.yml`). Locally, run the **narrowest check** that answers the question
you have right now, and only when you have one. This machine's CPU and memory are shared with
other agents, so each local cargo run has a real cost.

- Compiles? `cargo check -p <crate>`.
- Behaves? One test target, filtered to the test: `cargo test -p kestrel --test <file> <name>`.
  Each file in `crates/*/tests/` is its own binary, and the suite builds the supervisor and
  scripted agent itself, so a whole-package or workspace run is many builds, not one.
- Batch edits before checking; one check after a coherent change, never one per line.
- Before pushing, `cargo fmt --all`. Leave clippy, the workspace suite and the `#[ignore]`d
  suites to CI. CI runs only on a pull request, so finish by pushing the branch and opening one;
  then read what failed with `gh pr checks` and `gh run view --log-failed`.
- Documentation, skill and agent-guidance changes need no local check.

Where a skill says "the full test suite" or "the project's automated checks", in this repo
that means pushing and reading CI.

### Compatibility

Kestrel is an unreleased early prototype with no users. Do not preserve backward compatibility or
write data migrations for existing installations; change the original schema and migrations
directly until the project is released.

### Comments

**The default is no comment.** Write one only where a reader would otherwise get it wrong:
a trap, a rejected alternative, a constraint an ADR imposes, a consequence that is invisible
at the call site. One sentence. Prefer a better name to a comment explaining a worse one.

Delete on sight:

- Restatements of the code, including doc comments that paraphrase the signature below them.
- Roadmap narration (`At 0.1 it starts and waits; the link arrives in 0.1/04`). The issue
  tracker holds the plan; the code holds what is true now.
- Module docs that repeat the module's name.

Ten comments in a codebase is a reasonable number. Assume a comment is unnecessary and make
it argue for itself.
