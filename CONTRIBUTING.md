# Contributing

Start with Git, [mise](https://mise.jdx.dev/) and rustup installed. From a fresh checkout:

```sh
git clone https://github.com/openkestrel/kestrel.git
cd kestrel
mise trust
mise install
mise exec -- rustc --version
mise exec -- printenv CARGO_TARGET_DIR
mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  cargo test --locked -p kestrel --test suite \
  work::a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance -- --exact
```

That runs one supervisor-backed test without Docker, a browser or provider credentials.
`rust-toolchain.toml` pins Rust and its rustfmt/clippy components; `mise.toml` pins the other tools
and installs the repository's Git hooks. Use `mise exec --` below, or activate mise in your shell.
Verify that `CARGO_TARGET_DIR` is this checkout's **absolute** `target/`, never another checkout's
shared cache: nested helper builds change directories. For a separate cache, put
`env CARGO_TARGET_DIR=/absolute/path` after `mise exec --`.

Choose the narrowest check that answers your current question; batch related edits first:

| Question | Command after `mise exec --` |
| --- | --- |
| Does this crate compile and lint? | `cargo clippy -p <crate> --all-targets -- -D warnings` |
| Does this behavior pass? | `cargo test -p kestrel --test suite <module>::<name> -- --exact` |
| Which tests match? | `cargo test -p kestrel --test suite <module>:: -- --list` |
| Format before pushing | `cargo fmt --all` |

Ordinary control-plane tests live in `crates/kestrel/tests/suite/`, one module per area in one
binary. Drop `--exact` to match a substring or a module prefix. Each test owns its fixture under
plain `cargo test`; CI uses nextest with one process per test. Heavy suites such as `docker`,
`image` and `compose` stay separate (`--test docker`) and ignored for ordinary runs; CI selects
those requiring images. Leave full workspace and ignored suites to CI.

Direct `cargo test` prepares supervisor, scripted-agent and Client executables automatically with
Cargo freshness checks in the test binary's target/profile directory. No manual prebuild is needed;
nextest processes share preparation by invocation. Within an invocation, missing or replaced
prepared helpers fail loudly. `KESTREL_TEST_HELPERS` is for CI's prebuilt helper directory and bypasses local compilation.

Use `support::git::command` and `shell_exports` for Git fixtures and spawned scripts: they isolate
system/global Git config and supply fixture identities. The command above also isolates descendants
from personal signing settings. Declare common setup with `support::fixture::Fixture`; necessary
SQL fault injection and at-rest observation belong on `support::Database`.

Default builds retain workspace file/line debug information and omit dependency debuginfo. For
variables and types, use `cargo test --profile full-debug -p kestrel --test suite <module>::<name>`
or `cargo build --profile full-debug -p kestrel`. This recompiles into `target/full-debug`;
nextest spells the option `--cargo-profile full-debug`.

Read [architecture conventions](docs/architecture/conventions.md) and [agent guidance](AGENTS.md)
for contract generation, test seams and issue/PR rules. Documentation needs no local check. Push
and open a PR naming its issue (`Closes #<number>` against `main`); CI selects checks by changed
area and is the full merge gate. Inspect `gh pr checks` and `gh run view --log-failed`; record
unrelated failures or flakes before rerunning, as [documented](docs/agents/flakes.md).
