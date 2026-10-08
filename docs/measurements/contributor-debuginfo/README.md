# Debuginfo: the reduced default and the full-debug override

Measured on 2026-10-08 for [Reduce debuginfo with a full-debug override](https://github.com/openkestrel/kestrel/issues/592),
part of [Collapse the test harness's operation layer](https://github.com/openkestrel/kestrel/issues/474)
and the [contributor targets decision](https://github.com/openkestrel/kestrel/issues/495).

The workspace manifest builds workspace code with line-tables-only debuginfo and dependencies with
none, and adds a `full-debug` profile that inherits `dev` with full debug info for both. The
dependency override also sets `strip = "none"`: with debuginfo off, Cargo passes
`-C strip=debuginfo` on its own ([cargo#13257](https://github.com/rust-lang/cargo/pull/13257)), and
on macOS before rustc 1.98 that strip leaves a proc-macro dylib dyld refuses to load
([rust#157750](https://github.com/rust-lang/rust/issues/157750)).

## Method

One tree, both profiles, isolated empty target directories, so the only difference between the two
columns is the profile. Each row is one sample; this is not the
[#495](https://github.com/openkestrel/kestrel/issues/495) three-run median, which remains
[0.4/35](https://github.com/openkestrel/kestrel/issues/595)'s to record.

All commands ran through the baseline's prefix, against the toolchain `rust-toolchain.toml` pins:

```sh
mise exec -- env CARGO_BUILD_JOBS=2 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  cargo <command>
```

The recorded workloads are the baseline's, with the consolidation
([0.4/31](https://github.com/openkestrel/kestrel/issues/591)) applied: the one-test-binary
compilation and the focused test now name `--test suite` and
`work::a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance`, where the baseline named
`--test work`. That is a changed sequence, reported rather than compared with the baseline's
numbers. The override column adds `--profile full-debug` to the same commands.

Both columns were measured in the same session, on the baseline's host — Apple M2, 8 logical CPUs,
16 GiB RAM, macOS 27.0.1 (26A434) — with Rust and Cargo 1.96.0 selected by `mise`. The host is
shared with other agents, so the samples carry contention noise.

## Results

| Workload | reduced default | `--profile full-debug` | Difference |
| --- | ---: | ---: | ---: |
| Clean control-plane build | 88.54 s | 102.57 s | −14.03 s (−13.7 %) |
| One integration-test binary compilation | 52.19 s | 62.04 s | −9.85 s (−15.9 %) |
| First focused test | 36.61 s | 43.51 s | −6.90 s (−15.9 %) |
| Warm focused test | 1.96 s | 2.00 s | −0.04 s |
| No-op control-plane build | 0.33 s | 0.34 s | −0.01 s |
| Incremental control-plane build | 4.39 s | 4.55 s | −0.16 s |
| Target directory after all workloads | 3,235,460 KiB (3.09 GiB) | 5,311,672 KiB (5.07 GiB) | −2,076,212 KiB (−39.1 %) |

The first focused test builds the executable helpers as part of the workload, and passed under both
profiles (one test passed, 752 filtered); inside the test it took 33.22 s under the reduced default
and 41.98 s under the override.

Artifacts built by the focused test:

| Artifact | reduced default | `--profile full-debug` | Difference |
| --- | ---: | ---: | ---: |
| `kestrel-control-plane` | 67,719,528 B | 75,692,088 B | −7,972,560 B (−10.5 %) |
| `kestrel-supervisor` | 27,462,528 B | 30,380,144 B | −2,917,616 B (−9.6 %) |
| `kestrel-scripted-agent` | 17,064,232 B | 18,929,896 B | −1,865,664 B (−9.9 %) |
| `suite`, the test binary | 87,416,136 B | 96,225,096 B | −8,808,960 B (−9.2 %) |

The linked binaries shrink by about a tenth, less than the removed debuginfo alone suggests: the
`strip = "none"` that avoids the macOS breakage also leaves Cargo's implicit strip off, and that
strip's purpose is removing the precompiled standard library's debug info from linked outputs
([cargo#13257](https://github.com/rust-lang/cargo/pull/13257)).

[0.4/35](https://github.com/openkestrel/kestrel/issues/595) records the #495 medians. This one
sample sums the recorded empty-target sequence — clean build, test-binary compile, first focused
test — to **177.34 s**, above the 120 s first-test budget on this host; the same sequence under the
override sums to 208.12 s, so the reduced default removes 30.78 s (14.8 %) of Cargo time from it.
The budget stays open for investigation.

## What the two profiles compile

`flags-*.txt` records the `-C debuginfo=` and `-C strip=` flags Cargo passed, one line per crate,
from the same runs:

| Build | Workspace code | Dependencies |
| --- | --- | --- |
| `cargo build -p kestrel` (dev) | `-C debuginfo=line-tables-only`, 3 units (`kestrel`, `kestrel-control-plane`, `kestrel-operator-types`) | no `-C debuginfo`, 218 units |
| `cargo test -p kestrel --test suite --no-run` (test) | `-C debuginfo=line-tables-only`, 5 units (`suite` included) | no `-C debuginfo`, 78 units |
| `cargo build --profile full-debug -p kestrel` | `-C debuginfo=2`, 221 units | `-C debuginfo=2` |

No unit received `-C strip=`. The override reaches the helpers the harness builds for itself:
`built::named` selects the profile from the test binary's directory, so the first focused test under
`--profile full-debug` built `kestrel-supervisor` and `kestrel-scripted-agent` into
`target/full-debug`, and the sizes above come from those artifacts.

## What this does not change

`.config/nextest.toml` and its `ci` and `stress` profiles, the CI workflow's selectors and its
rust-cache configuration, and the toolchain pin are untouched; no linker, cache or build tool is
adopted.

## Reproducing

```sh
bash docs/measurements/contributor-debuginfo/measure-debuginfo.sh /tmp/kestrel-debuginfo
```

The script needs `mise`, the pinned toolchain, and `/usr/bin/time`; it writes `results.txt` and the
`flags-*.txt` tables, deletes its target directories, and restores the checkout. The `results.txt`
committed here is that script's output.
