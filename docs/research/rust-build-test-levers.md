# Build and test time levers for kestrel's many-binary workspace

Research checked 3 October 2026 against the Cargo book, the nextest docs, the sccache, Swatinem/rust-cache, mold and cargo-hakari READMEs and docs, the Rust blog, and matklad's posts. Nothing here was measured on kestrel. The kestrel facts come from reading the tree at `1365b23`, and the "what it buys" figures are the sources' numbers for other projects.

## Kestrel's shape

- **Four crates.** `kestrel` (lib + `kestrel-control-plane` bin, ~27.8k lines in `src/`), `kestrel-client`, `kestrel-supervisor`, `kestrel-scripted-agent` (`Cargo.toml`). Rust 1.96.0 (`rust-toolchain.toml`). There is no `[profile.*]` section and no `.cargo/config.toml`.
- **59 integration-test binaries in `crates/kestrel/tests/`**, plus 3 in `crates/kestrel-supervisor/tests/`. 58 of the 59 declare `mod support;`, so each one compiles the 5,746-line harness (`tests/support/`, 19 submodules) and links against the `kestrel` lib and its whole dependency graph: axum, sqlx/sqlite, reqwest+rustls, tokio, minijinja, tracing. There are about 760 `#[test]`/`#[tokio::test]` functions across them, and about 37.5k lines of test code (#474).
- **Nested `cargo build` from tests.** `tests/support/built.rs` runs `cargo build --package <p>` for `kestrel-supervisor`, `kestrel-scripted-agent` and `kestrel-client`, behind a per-process `OnceLock`. `CARGO_BIN_EXE_<name>` covers only the testing package's own binaries ([Cargo env vars](https://doc.rust-lang.org/cargo/reference/environment-variables.html)), so it can stand in only for `kestrel-control-plane`, and `roles.rs` and `cli.rs` already use it that way.
- **CI** (`.github/workflows/ci.yml`, `workspace` job): `cargo fmt --check` → `cargo clippy --workspace --all-targets` → `cargo build --workspace` → `cargo nextest run --workspace --profile ci` (retries 2, `.config/nextest.toml`). Swatinem/rust-cache restores a `rust-dependencies` key that only `main.yml`'s `rust-cache` job writes (clippy + build + `cargo test --no-run`). The `#[ignore]`d Docker suites run with plain `cargo test --test <name> -- --ignored` in separate jobs, and each of those jobs compiles the test binaries again.

## 1. Consolidating test binaries

**Mechanism.** Cargo compiles each file in `tests/` as its own crate and links it into its own executable ([matklad, "Delete Cargo Integration Tests"](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)). Each of the 58 harness-using binaries therefore re-parses, re-type-checks and re-codegens `support/`, then links a full executable against every dependency. Matklad recommends a single `tests/it/main.rs` with the suites as modules.

**What it buys, measured elsewhere.** On Cargo's own test suite the change cut test compile time **3×** and on-disk artifacts **5×**. In one of matklad's projects, wall-clock test time went from **20 s to 13 s**, because `cargo test` runs binaries one after another and "the critical path is the sum of longest tests in each binary" ([same post](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)). Matklad's general advice is to cut the number of final artifacts because linking is repeated work ([fast-rust-builds](https://matklad.github.io/2021/09/04/fast-rust-builds.html)).

**How it plays out for kestrel.**
- The run-time half of the argument mostly does not apply to the main gate. Nextest already runs every test in its own process across all binaries ([how it works](https://nexte.st/docs/design/how-it-works/)), so serial binaries are not the critical path. The compile and link half applies in full: 59 links shrink to about 1 (or a few), and the harness compiles once instead of 58 times.
- The trade-off is edit granularity. Today, editing one suite rebuilds one small crate. In a merged binary, editing any suite recompiles the whole merged test crate (~37.5k lines). Incremental compilation softens this locally, but CI runs with `CARGO_INCREMENTAL=0` because rust-cache sets it ([rust-cache](https://github.com/Swatinem/rust-cache)). CLAUDE.md's narrow check `cargo test -p kestrel --test <file> <name>` becomes `cargo test -p kestrel --test it <module>::<name>`. The narrow check would then cost one big compile rather than one small one.
- **Middle option: harness as a library crate.** Move `tests/support/` into a `kestrel-test-support` crate used as a dev-dependency. The harness then compiles once, and each of the 59 suites keeps only its own code and its link. This removes the 58× harness compile but not the 58× link. The crate's `pub` items no longer need `#![allow(dead_code)]`, though rot then hides behind `pub` instead. The idea is related to #474's narrowing of that interface but separate from it.
- **Grouping:** the `#[ignore]`d Docker suites (`image`, `docker`, `opencode`, `development`, `control_plane`, `compose`, `cleanup`, `conformance`) are selected with `--test <name>` in CI jobs. They can stay as separate binaries or be grouped into a second `docker` binary that CI filters by module path. Either way the fast gate's binaries collapse to one.

## 2. cargo-nextest

**Already in use** in the `workspace` job (`mise.toml` pins 0.9.146).

- **What it buys for test run time.** On the maintainers' benchmark (16-core 7950X), nextest ran 1.4×–3.4× faster than `cargo test`, e.g. omicron 444 s → 203 s and tokio 24.3 s → 11.6 s. Build time is explicitly excluded from these numbers ([benchmarks](https://nexte.st/docs/benchmarks/)).
- **Cost: one process per test.** Process creation is cheap on Linux but can be slow on macOS when security tooling interferes. Shared in-memory state (caches, semaphores, a shared server) does not carry between tests ([why process-per-test](https://nexte.st/docs/design/why-process-per-test/)). **Kestrel-specific consequence (from reading the code, not measured):** `built.rs`'s `OnceLock` resets in every process, so each of roughly 760 tests that needs the supervisor, scripted agent or client spawns its own `cargo build --package …`. Each call is a no-op build that still resolves the workspace, fingerprints, and takes Cargo's build-directory lock. With nextest running many tests in parallel, those calls queue on the lock.
- **Lever: build the helper binaries once.** A nextest **setup script** can run before matching tests and export variables to them through `$NEXTEST_ENV` ([setup scripts](https://nexte.st/docs/configuration/setup-scripts/)). The script would build the three binaries once and export their paths, and `built.rs` would read the variable and fall back to building when it is unset. The feature is still **experimental** (`experimental = ["setup-scripts"]`). The stable equivalent, artifact dependencies (`artifact = "bin"`, `CARGO_BIN_FILE_*`), is **nightly-only** behind `-Z bindeps` ([Cargo unstable](https://doc.rust-lang.org/cargo/reference/unstable.html#artifact-dependencies)).
- **Lever: archive and partition** ([partitioning](https://nexte.st/docs/ci-features/partitioning/)). `cargo nextest archive` lets one job build and several jobs run `--partition hash:m/n` on the same build. That helps when test run time dominates, not compile time. Kestrel's Docker jobs (`image`, `development`, `control-plane`, `compose`) each recompile with `cargo test`. Running them from a nextest archive built once would remove those repeated compiles.

## 3. cargo-hakari (workspace-hack)

**Mechanism.** Cargo unifies features per build invocation, so a dependency gets different feature sets, and therefore different artifacts, depending on which packages are being built. Everything that depends on it is rebuilt to match. Hakari generates a `workspace-hack` crate that every member depends on, pinning one unified feature set ([hakari about](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html)).

**What it buys.** "Individual commands can be anywhere from 1.1x to 100x faster", and "a cumulative performance benefit of up to 1.7x" for "moderately large workspaces" ([same](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html)). Costs: first builds can be slower, a crate can silently rely on features only the hack enables, publishing gets harder, and `cargo hakari generate` has to be re-run.

**How it plays out for kestrel.** The workspace is small, but its feature sets diverge in exactly the way hakari targets:
- `serde_json` gets `preserve_order` only in `kestrel-client`.
- `tokio` features differ per crate (`rt-multi-thread`+`signal` in kestrel, `fs`/`process`/`io-util` in the supervisor).
- `reqwest` is `json`+`rustls` in kestrel and client but `json`+`stream` in the supervisor.

So `cargo test -p kestrel` (CLAUDE.md's narrow check), the nested `cargo build --package kestrel-client`, and CI's `--workspace` build each resolve a different feature set. Each one compiles its own copy of `serde_json`, `tokio`, `reqwest`, `axum` and the crates downstream of them into the shared target dir. That is extra local compile work, and the cache that `main.yml` writes with `--workspace` partly misses for narrow `-p` builds.

There are two lighter alternatives to hakari:
- Cargo's `[resolver] feature-unification = "workspace"`, which Cargo's build-performance guide recommends but which is still **nightly-only** ([Cargo build performance](https://doc.rust-lang.org/cargo/guide/build-performance.html)).
- Aligning the feature lists by hand in `[workspace.dependencies]`, for example by giving every crate the same `tokio`/`reqwest`/`serde_json` features.

With four crates, aligning by hand gets most of the benefit without a generated crate.

## 4. sccache and Actions caching

**Swatinem/rust-cache (in use).** It caches `~/.cargo` and the *dependency* artifacts in `target/`. It removes incremental artifacts and anything older than a week, and "the workspace crates themselves are not cached since doing so is generally not effective". It sets `CARGO_INCREMENTAL=0`. `cache-workspace-crates: true` opts in to caching workspace crates. The key hashes the rustc version, `Cargo.lock`/`Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml` and every env var prefixed `CARGO`, `CC`, `CFLAGS`, `CXX`, `CMAKE` or `RUST` ([rust-cache README](https://github.com/Swatinem/rust-cache)). This matches matklad's advice: "cache all the dependencies, but don't cache project's own crates", don't use incremental in CI, and turn off debuginfo to keep the cache small ([fast-rust-builds](https://matklad.github.io/2021/09/04/fast-rust-builds.html)).

**What sccache adds on top.** Very little for kestrel:
- "Crates that invoke the system linker cannot be cached. This includes `bin`, `dylib`, `cdylib`, and `proc-macro` crates", and "Incrementally compiled crates cannot be cached" ([sccache README](https://github.com/mozilla/sccache)). All 62 test binaries and 4 bins link, so none of them can be cached. Dependencies are already cached by rust-cache. That leaves only the `kestrel` lib crate, which every PR changes anyway.
- The GHA backend (`SCCACHE_GHA_ENABLED=on`) silently skips stores when it hits the rate limit ([GHA.md](https://github.com/mozilla/sccache/blob/main/docs/GHA.md)).

Locally, sccache could share dependency builds across sibling worktrees. However, those worktrees already share one `CARGO_TARGET_DIR` (`mise.toml`), which covers most of that.

**Constraint on any CI env-var lever.** A `CARGO_*` variable set only in `ci.yml` changes rust-cache's key. `main.yml`'s writer job must therefore set the same variables, or every gate run restores nothing.

## 5. Linkers: lld and mold

- **lld is already the default for kestrel's CI.** Since Rust 1.90, `x86_64-unknown-linux-gnu` links with lld by default. On ripgrep this cut linking 7× in an incremental rebuild (−40% end to end) and gave about −20% on a from-scratch debug build ([Rust blog](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable/)). CI is `ubuntu-latest` on 1.96, so the "switch to lld" lever is already used.
- **mold** is the remaining linker lever on Linux. Its README reports it is "4.9x faster than LLVM lld" at the median, e.g. Chromium 16.6 s → 1.65 s on a 64-core Threadripper ([mold](https://github.com/rui314/mold)). Configure it via `.cargo/config.toml` with `-C link-arg=-fuse-ld=mold` ([Cargo build performance](https://doc.rust-lang.org/cargo/guide/build-performance.html)). The gain scales with the number of links, so with 59+ test binaries it is relatively large today and shrinks after consolidation. A 4-core `ubuntu-latest` runner also gets less from mold's parallelism than the benchmark hardware.
- mold's README documents Linux only, so it does not help the macOS machines where agents run local checks. macOS linker options were not researched here.

## 6. `profile.dev` debuginfo

The dev profile defaults to `debug = true` (full), `incremental = true`, `codegen-units = 256`, and `split-debuginfo = "unpacked"` on macOS ([Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). Cargo's build-performance guide recommends:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true
```

This gives "faster code generation", "faster link times" and a smaller `target/`, at the cost of a rebuild under `--profile debugging` when you need a real debugger ([Cargo build performance](https://doc.rust-lang.org/cargo/guide/build-performance.html)). Panic backtraces keep file and line. For kestrel, every one of the 59 links is dominated by the debuginfo of axum/sqlx/rustls/tokio, so dropping dependency debuginfo shrinks every link and every rust-cache upload. The guide gives no numbers. The two Cargo.toml profiles are part of rust-cache's key, so changing them invalidates the cache once.

One kestrel-specific trap: `built.rs` maps the test binary's profile directory back to a `--profile` (`debug` → `dev`). A custom `debugging` profile builds into `target/debugging/`, which that mapping already handles.

## Ranking for kestrel (judgment, not measurement)

1. **Debuginfo profile** (§6): a few lines in one file, cheaper compiles and links everywhere, little downside.
2. **Consolidate the test binaries** (§1), or at least move the harness into a crate. This is the largest structural lever: it removes 58 harness compiles and most of the 59 links. Do it together with #474, since both touch every suite.
3. **Stop the per-test nested `cargo build`** (§2): a nextest setup script, or a pre-built path passed in by env var, with `built.rs` falling back to building when unset.
4. **Align dependency features by hand** (§3) before reaching for hakari.
5. **mold in CI** (§5): worth trying after 1 and 2, and measure it, because lld is already the baseline.
6. **sccache** (§4): skip. rust-cache already covers what sccache can cache here.

To measure any of these, use the same approach throughout: split `--no-run` from the run, as matklad recommends, and read `cargo build --timings`.
