# Contributor build, test and CI baseline

Measured on 2026-10-04 for [Measure the contributor build, test and CI baseline](https://github.com/openkestrel/kestrel/issues/489), part of [Map: Plan 0.4 — kestrel starts from one command](https://github.com/openkestrel/kestrel/issues/481). These are observations for the contributor-targets decision, not chosen targets or tooling changes.

## Local workloads

Commit `499f7d1474b9da442f5a30b8fd0c669647c00783`; Apple M2, 8 logical CPUs, 16 GiB RAM, arm64; macOS 27.0.1 (26A434). `mise exec` selected Rust 1.96.0 (`ac68faa20`) and Cargo 1.96.0 (`30a34c682`); mise 2026.9.14. Cargo used two compile jobs and the default dev/test profiles, with unoptimized code and debuginfo. The host is shared. Each row is one sequential sample; this is not a statistical benchmark.

A disposable clone had an initially empty, checkout-local `target/`. The existing Cargo registry and installed toolchain were reused: **clean means compiled artifacts are absent, not an empty machine or empty download cache**. No workspace suite ran locally. The original workspace's 581 MiB target was left alone.

All Cargo commands used this prefix:

```sh
mise exec -- env CARGO_BUILD_JOBS=2 \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 cargo
```

| Workload, in measurement order | Command after prefix | Wall time | Conditions |
| --- | --- | ---: | --- |
| Clean control-plane build | `build --locked -p kestrel` | 89.77 s | Empty target; builds the control-plane package and dependencies, not the entire workspace |
| One integration-test binary compilation | `test --locked -p kestrel --test work --no-run` | 48.19 s | Reuses clean-build artifacts; test-only dependencies and test binary still absent |
| First focused test | `test --locked -p kestrel --test work a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance -- --exact` | 42.974 s | Test binary built; supervisor and scripted-agent executable artifacts absent at start |
| Warm focused test | Same focused command | 1.789 s | All artifacts present; fresh test process and fixtures |
| No-op control-plane build | `build --locked -p kestrel` | 0.237 s | No source change, all artifacts present |
| Incremental control-plane build | `build --locked -p kestrel` | 4.065 s | One newline appended to `crates/kestrel/src/lib.rs`; original bytes restored afterward |

Both test runs passed exactly one test and filtered 31. The test runner reported 41.71 s on the first run and 1.31 s warm; Cargo reported 0.49 s and 0.27 s compilation respectively. These runner durations include any helper work inside the test, not just assertions. `support::built::named` calls Cargo at runtime; `supervisor::binary` and `scripted_agent::binary` cache the result only within a test process. The first/warm difference therefore includes helper compilation. Its precise breakdown was not separately instrumented.

The incremental sample is a whitespace edit, a lower-cost bound rather than a prediction for changing behavior, public types or dependency features. The no-op row is not called an incremental edit. A clean full-workspace local build and a cold one-test build directly from empty target were not measured; the scoped sequence above supplies the baseline without an unnecessary workspace build.

| Target size (`du -sk`, allocated KiB) | KiB | GiB |
| --- | ---: | ---: |
| After clean control-plane build | 1,634,692 | 1.56 |
| After compiling `work` | 2,734,040 | 2.61 |
| After focused runs and incremental build | 3,819,740 | 3.64 |

The original workspace's 581 MiB is an uncontrolled pre-existing cache, not a comparable clean-build footprint. The disposable target was not shared with another checkout.

Raw observations: [environment and sizes](local-environment.json), [clean compilation](clean-build.log), [test compilation](test-build.log), [follow-up timings and exact commands](local-followups.json), [first run](first-test.log), [warm run](warm-test.log), [no-op](noop-build.log), [incremental edit](whitespace-edit-build.log). [The follow-up measurement script](measure-followups.py) runs from this repository location after the build and `--no-run` commands above; it records timings and restores the edited file even on failure. It requires a disposable checkout and writes fresh results over its reports.

## Fresh-clone walkthrough

The measured clone was local (`git clone --no-hardlinks --no-checkout` from the existing checkout, then detached checkout of the measured commit), taking 0.16 s for the clone. That measures local Git object copying, not GitHub download time. A network clone, fresh Rust/mise installation and a cold registry download are **unmeasured**.

For a contributor with Git, mise and the pinned Rust toolchain installed:

```sh
git clone https://github.com/openkestrel/kestrel.git
cd kestrel
git checkout --detach 499f7d1474b9da442f5a30b8fd0c669647c00783
mise trust
mise exec -- rustc --version
mise exec -- cargo --version
mise exec -- printenv CARGO_TARGET_DIR
mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 cargo build --locked -p kestrel
mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 cargo test --locked -p kestrel --test work --no-run
mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null \
  GIT_CONFIG_NOSYSTEM=1 cargo test --locked -p kestrel --test work \
  a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance -- --exact
```

To reproduce the follow-up measurements, replace the final focused-test command above with the following. The measured commit predates this report and script, so fetch the artifact PR and restore only its script; the Rust source remains at the pinned commit. This additionally requires Python 3.

```sh
git fetch origin refs/pull/518/head
git restore --source=FETCH_HEAD -- docs/measurements/contributor-baseline/measure-followups.py
python3 docs/measurements/contributor-baseline/measure-followups.py
```

The measured Cargo portion from empty target to a first passing test is **180.934 s (3m01s)**: 89.77 + 48.19 + 42.974. Including the measured local clone adds 0.16 s. This is summed command time, excluding trust/version checks, checkout, human pauses and this session's investigation gaps; it is not end-to-end onboarding wall time. The explicit build and `--no-run` steps separate the costs; contributors can invoke the focused test directly, but that path was not separately timed. This test needs no Docker, browser or provider sign-in.

Traps verified against this checkout:

- **The shell may bypass the pin.** Plain `rustc` and `cargo` here resolved to Homebrew 1.98.1, while `mise exec` resolved 1.96.0. Merely having mise installed does not activate its environment. `rust-toolchain.toml` pins Rust; `mise.toml` enables reading it. On a machine missing the pin, install rustup/Rust 1.96.0 and its rustfmt/clippy components before measuring. Installation time is not included here.
- **A new clone needs mise trust.** Trust the repo configuration before using it; in this run tools were already installed, so no installation was timed. Full Client work additionally needs the pinned Bun and Caddy tools, whereas this Rust test does not.
- **Target paths must agree and be absolute.** `mise.toml` sets `CARGO_TARGET_DIR` to `{{config_root}}/target`, which resolved to `/private/tmp/kestrel-contributor-baseline/checkout/target`. Runtime helper builds must land alongside the test executable. An inherited target from another checkout both contaminates the measurement and can cause Cargo lock contention; a relative target is unsafe when nested builds change directories. Verify the resolved value before running, and use a disposable target rather than cleaning shared artifacts.
- **Git signing can escape into fixtures or spawned scripts.** The selected repository fixture already uses `support::git::command`, disabling global/system config and providing fixture identities; shell environments also receive its exports. That protection is present, so the old blanket warning about every fixture inheriting signing is too broad. The measured command additionally disabled global/system config for descendants. For new fixture code, use the helper and check any git spawned through a harness script; no deliberately broken signing setup was run here.
- **A precompiled test is not necessarily ready to run cheaply.** `--no-run` builds the test target, but the first selected test still builds executable helpers. A second process calls Cargo again even when no compilation remains. This distinction matters when comparing `cargo test` and nextest's one-process-per-test execution.

## CI baseline

Primary executed run: [CI on `a6b29919f4263307316695a37929d821a02cdd0a`](https://github.com/openkestrel/kestrel/actions/runs/37169733884), a pull request run. All executed jobs used GitHub-hosted `ubuntu-latest`, resolved in the logs to Ubuntu 24.04, runner image `20260927.320`, x86_64, Rust 1.96.0. The logs' Docker information reports 4 CPUs for image-build runners; the Rust runner's exact CPU model, memory and instantaneous competing load were not logged. Do not compare this runner directly with the local M2.

Caches were warm: mise restored its cache; the Rust job restored a **545 MiB compressed** rust-cache archive via a fallback key (not an exact lockfile-key match), including registry and target paths. This archive size is not an uncompressed `target/` measurement. Client restored Bun's cache. Image build layers were cacheable. These are observed warm/fallback-cache CI durations, not cold CI build measurements.

| Job | Queue after job creation | Job execution wall | Second executed run | Third executed run |
| --- | ---: | ---: | ---: | ---: |
| changes | 4 s | 6 s | 4 s | 5 s |
| kestrel-env (build) | 4 s | 125 s | 125 s | 115 s |
| workspace (rust) | 3 s | 405 s | 523 s | 499 s |
| kestrel-client (build) | 3 s | 39 s | 27 s | 40 s |
| kestrel (build) | 3 s | 225 s | 186 s | 223 s |
| client | 3 s | 165 s | 184 s | 135 s |
| images (arm64) | — | Skipped | Skipped | Skipped |
| kestrel-dev (image) | 3 s | 106 s | 98 s | 93 s |
| kestrel-env (image) | 3 s | 179 s | 173 s | 179 s |
| kestrel (image) | 3 s | 96 s | 92 s | 84 s |
| kestrel (compose) | 3 s | 165 s | 173 s | 166 s |
| gate | 3 s | 5 s | 5 s | 3 s |

Second run: [CI on `3d089a04ad8a5a7ff0300d7ae369ded75feb915c`](https://github.com/openkestrel/kestrel/actions/runs/37168367300). Third run: [CI on `16c12942b98edd2c3debc354913b842bbc797b3e`](https://github.com/openkestrel/kestrel/actions/runs/37137653985). Their per-job timestamps and runner labels were sampled; cache logs were inspected only for the primary run. The different commits/workloads make this observed variability, not repeated trials of one tree. Arm64 was skipped in all three; its duration remains unmeasured.

For the primary run, workflow creation to completion was **449 s (7m29s)**, while earliest job start to gate completion was **422 s (7m02s)**. The initial 27 s includes workflow preparation and queueing; the first job's own `created_at`→`started_at` queue time was 4 s. Later jobs' waiting for prerequisites is separate from their 3–4 s job queue time. Executed job durations sum to **1,516 s (25m16s)**, but jobs run in parallel: that sum is not user waiting time. Job timestamp resolution is one second.

Rust was the primary run's critical path:

| Rust step | Step timestamp duration | More precise log observation |
| --- | ---: | --- |
| mise setup | 9 s | Cached Rust 1.96.0 |
| rust-cache restore | 10 s | Fallback cache key; 545 MiB compressed |
| Formatting | 2 s | `cargo fmt --all --check` |
| Lint | 33 s | Cargo reported 32.85 s |
| Build workspace binaries | 19 s | Cargo reported 19.27 s; follows lint and cache restore |
| Test | 327 s | Cargo test compilation 1m19s, then nextest summary 245.014 s; 1,432 passed, 53 skipped |

The 19 s CI build is neither a clean compile nor an isolated incremental source-edit benchmark: it follows clippy and restored dependency artifacts. The test step spends about a quarter compiling and three quarters executing tests; runtime includes helper Cargo invocations. No per-helper contention breakdown was captured.

[The latest green merge-group run on the local measured commit](https://github.com/openkestrel/kestrel/actions/runs/37229645631) took **14 s** creation→completion but reused a passing tree under ADR-0042: only `changes` and `gate` ran. It is excluded from the executed-gate baseline.

Raw timestamp evidence: [primary run](ci-run.json), [primary jobs and steps](ci-jobs.json), [second jobs](ci-jobs-second.json), [third jobs](ci-jobs-third.json), [reused run](reused-run.json), [reused jobs](reused-jobs.json). Fetch the primary logs again with:

```sh
gh run view 37169733884 --repo openkestrel/kestrel --log
```

## What the next decision can use

The controlled local sequence reaches one passing integration test in roughly three minutes of Cargo command time, while a warm repeat is under two seconds. That gap includes compilation of the test and its runtime helpers. The observed executed Rust CI jobs take 405–523 s and are the slowest jobs in each sample. The next contributor-targets ticket can choose separate budgets for clean setup, a warm focused test and an executed gate using these named workloads. No lever or target is selected here.
