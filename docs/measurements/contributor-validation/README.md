# Contributor slice validation (#595)

Measured 2026-10-31 against `e800a9bd70f18097037a33d9e8c693cdbcfb1b3e`,
for the [#495 contract](https://github.com/openkestrel/kestrel/issues/495) and
[amended harness spec](https://github.com/openkestrel/kestrel/issues/474).
The [baseline](../contributor-baseline/README.md) remains intact.

## Conditions and reproducibility

Local: Apple M2, 8 logical CPUs, 16 GiB RAM, macOS 27.0.1 (26A434), arm64;
Rust 1.96.0 (`ac68faa20`), Cargo 1.96.0 (`30a34c682`), mise 2026.9.14.
This is the baseline's host, toolchain and two-job limit, with shared-host contention. A 15:02 UTC load snapshot during trial three was
15.84 / 15.86 / 14.70 (1/5/15-minute averages), above the eight logical CPUs.
Host load was not controlled, so these results do not isolate a harness speedup.
Installed tools and registry downloads are reused. Each after trial starts with an empty,
absolute `/private/tmp/kestrel-595-target`; trials run sequentially and remove only this disposable
target. No full local workspace suite runs. Measurements exclude installation and download time.

[measure.py](measure.py) records exact commands, timestamps, exit codes and wall seconds in
[local.json](local.json), with per-command logs beside it. Run from the trusted current checkout:

```sh
git clone --no-hardlinks --no-checkout . /private/tmp/kestrel-595-before
git -C /private/tmp/kestrel-595-before checkout --detach 499f7d1474b9da442f5a30b8fd0c669647c00783
mise exec -- python3 docs/measurements/contributor-validation/measure.py
```

The script refuses an existing measurement target. It appends a newline to the selected test
module and restores the original bytes in `finally`; run without concurrent source edits.
The baseline clone is only for focused-edit comparison, prepared once before three warm/edit pairs.
It uses the same installed toolchain, registry, target path and compile-job limit as the after runs.

## Workloads

The baseline's **exact** empty-target sequence is preserved in its report:

```sh
cargo build --locked -p kestrel
cargo test --locked -p kestrel --test work --no-run
cargo test --locked -p kestrel --test work \
  a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance -- --exact
```

Its recorded single sample totals 180.934 s, with a 1.789 s warm repeat; neither is a three-run
median. This validation reports a **changed sequence separately**, because the original `work`
binary no longer exists: the latter two commands use `--test suite`, and the focused name gains
`work::`. All commands additionally name an explicit manifest path and target (see raw JSON).
The clean package build, test compilation, first helper-backed test and warm-repeat order is kept.
Sum the first three durations **within each trial**, then take the median of those totals.
Do not silently compare compilation of the consolidated suite with compilation of the old work binary.

The new focused-edit workload appends one newline to `tests/work.rs` before consolidation and
`tests/suite/work.rs` after it, then runs the corresponding exact focused command. It measures
recompilation plus one test and helper freshness checks; it is a whitespace-edit lower bound,
not a forecast for behavior or public-type edits. The commits differ by the full intervening
history, so the before/after difference does not isolate consolidation's causal cost.

## Local results (seconds)

| Workload | Trial 1 | Trial 2 | Trial 3 | Median | Budget |
| --- | ---: | ---: | ---: | ---: | --- |
| Clean package build | 121.635 | 191.953 | 138.216 | 138.216 | Component only |
| Compile suite after build | 92.497 | 130.996 | 84.768 | 92.497 | Component only |
| First focused invocation after compile | 37.497 | 58.913 | 58.347 | 58.347 | Component only |
| Empty target → first passing test (sum per trial) | 251.629 | 381.862 | 281.331 | **281.331** | **Miss: ≤120** |
| Warm focused repeat | 2.077 | 2.009 | 2.073 | **2.073** | **Pass: ≤3** |
| Focused repeat after test-module newline | 6.500 | 9.398 | 9.899 | **9.398** | Comparison workload |

| Original topology workload | Trial 1 | Trial 2 | Trial 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Warm focused repeat | 1.979 | 2.920 | 2.943 | 2.920 |
| Focused repeat after test-module newline | 3.904 | 3.045 | 3.073 | **3.073** |

The original-topology preparation took 164.945 s and is excluded from the warm/edit medians.
Each before invocation passed one test and filtered 31. The focused-edit median rises from
3.073 s to 9.398 s (+6.325 s, about 3.06×) across the recorded before/after trees. This is an
observed regression to investigate, not evidence that consolidation alone caused it: both source
history and debug profiles differ, and host contention varied. It has no separate agreed budget.

Each focused invocation passed exactly one test and filtered 1,044. The helper artifacts were
absent at the start of each first invocation. All three empty-target trials miss 120 s. Even
the clean package-build median alone exceeds it; test compilation and helper preparation add
more time. Investigate package compilation and consolidated-suite compilation under controlled
host load before attributing the miss or choosing another lever. No budget is weakened, and
[#595](https://github.com/openkestrel/kestrel/issues/595) / the contributor slice remain open.
The 300 s CI per-job observations below do not offset the local first-test miss.

## CI: changed topology

Three executed PR runs (all successful; no reused merge-group gates):

| Run | Commit | lint | helpers | archive | shard 1 | shard 2 | shard 3 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| [41166849641](https://github.com/openkestrel/kestrel/actions/runs/41166849641) | `b01b1ca71472d0e3360134309e7943432ec4621c` | 94 | 132 | 148 | 99 | 82 | 90 |
| [41166284310](https://github.com/openkestrel/kestrel/actions/runs/41166284310) | `9130f992f42e17ee1e04d8166c1faf0e537076e5` | 90 | 132 | 140 | 100 | 98 | 77 |
| [41166212557](https://github.com/openkestrel/kestrel/actions/runs/41166212557) | `b506f3339eb29ca95fff15c2b2f8f33b82daeee5` | 69 | 139 | 151 | 90 | 79 | 76 |
| Median seconds | | 90 | 132 | 148 | 99 | 82 | 77 |

Durations are `completed_at - started_at` at one-second resolution, including setup/teardown,
excluding job queue time and prerequisite waits. Each median is below the 300 s **per-job** budget.
The current workflow splits lint, archive construction and three nextest shards; these rows are
not equivalent to the baseline's monolithic 405/523/499 s Rust job. The median across each run's
slowest Rust job is 148 s. Archive and shards are sequential stages, not one 148 s end-to-end gate.
These are three nearby executed trees, matching the baseline's observational CI sampling method,
not three reruns of one tree or proof of a causal speedup. The runs execute 1,545 / 1,540 /
1,540 tests respectively ([summaries](ci-tests.txt)); their source refactors and five-test
difference are another comparability limit. Same-tree repeated CI timing remains unmeasured.

All three used GitHub-hosted `ubuntu-latest`, resolved to Ubuntu 26.04 image `20261025.179`, x64,
Rust 1.96.0. CPU model, RAM and instantaneous load are unavailable in these logs. Unlike the
baseline's Ubuntu 24.04 runner, this is a newer runner image. Rust compilation jobs restored the
same exact warm `v0-rust-rust-dependencies-Linux-x64-16801175-ec671d64` cache (595,197,285 B
compressed). Test shards extracted their run's archive instead of compiling or restoring a Rust
cache. Mise cache hits vary and tool setup is included in job time. Thus cache class is comparable
(warm Rust dependencies), while OS, topology and commits differ and are explicitly recorded.

Raw `ci-<run>.json` files hold jobs, steps and runner names; `run-<run>.json` records commit and
workflow identity. [ci-cache.txt](ci-cache.txt) captures relevant runner, compiler and cache lines
from all three logs. Reproduce with `gh api repos/openkestrel/kestrel/actions/runs/<run>/jobs`
and `gh run view <run> --repo openkestrel/kestrel --log`.

## Harness and selection evidence

The new first-test trials start without helpers and invoke plain `cargo test`, requiring automatic
preparation. All three sampled CI helper jobs passed the existing
`.github/scripts/test-helper-preparation.sh`: its cargo/nextest modes check once-per-invocation
preparation, deleted-artifact rebuilding, selected-target isolation and touched-source freshness;
its artifact/provenance/inplace modes check missing or replaced prepared artifacts fail, and a new
invocation replaces a hand-placed executable. Captured `ci*-helpers.txt` excerpts show the expected
stale/missing diagnostics inside that passing negative check; these are intentional failures.

The representative plain-Cargo fixture check is separate from timing trials:

```sh
mise exec -- env CARGO_TARGET_DIR=/private/tmp/kestrel-595-target CARGO_BUILD_JOBS=2 \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  cargo test --locked -p kestrel --test suite work::a_session_that_ -- --test-threads 2
```

[fixture-isolation.log](fixture-isolation.log) records the two concurrent lifecycle tests:
`work::a_session_that_reaches_an_instance_starts_and_ends_in_the_transcript` and
`work::a_session_that_ended_is_never_claimed_again`. Each uses its own control-plane and
repository fixture; nextest is not involved in this check. Both passed (2 passed, 0 failed,
1,043 filtered out), with the default per-test fixture isolation and two libtest threads.

[consolidation-inventory.json](consolidation-inventory.json) compares the original consolidation
commit `12d1e35b` with its parent: 802 `test`/`tokio::test` attributed function names on both sides,
no additions or removals, preserving duplicate-name multiplicity. This is a source inventory,
not a proof of behavioral coverage. The focused test keeps its name with the `work::` module
prefix. Heavy binaries remain independently selectable and ignored as described in conventions;
this documentation change alters no tests, ignore annotations, filters, fixture implementation or
CI selectors. Later additions explain the current suite's larger test count. The three sampled
CI runs execute the workspace through nextest's existing process-per-test isolation.

## Environment interruption

Before the controlled trials, a sandboxed attempt compiled the package and suite, then failed
before running the selected test: `built.rs` could not read Cargo's parent's start time
(`PermissionDenied`, macOS sandbox). [sandbox-attempt/local.json](sandbox-attempt/local.json)
and accompanying logs preserve that attempt. It is excluded from every median; the controlled
trials run with process metadata and local-listener access. This is an environment restriction,
not a test flake, and no retry of a behavioral failure is hidden.
