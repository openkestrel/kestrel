# Embedded jq (`--jq`) dependency and build cost

Measured on 2026-10-12 for [0.4/39 — Filter complete CLI responses with embedded jq
expressions](https://github.com/openkestrel/kestrel/issues/601). This is an observation of what
pinning jaq adds to the Client's build, not a chosen target or a contributor-benchmark change.

## What was pinned

`crates/kestrel-client/Cargo.toml` pins exact versions so a moved filter cannot arrive as a patch
release:

```toml
jaq-core = "=3.1.1"
jaq-json = { version = "=2.0.3", features = ["serde"] }
jaq-std = "=3.0.3"
```

`serde` on `jaq-json` is what converts a response the Client already holds as a `serde_json::Value`
into a `jaq_json::Val` without re-serialising it. `jaq-std` keeps its default features
(`std`, `format`, `log`, `math`, `regex`, `time`), which is what makes `jaq_std::funs` — and so the
standard library's `map`, `select`, `length`, `tojson` and the rest — available to an expression.
`jaq-core`'s own default (`std`) is kept.

## New packages

`git diff` of `Cargo.lock` against `main` adds ten packages:

```
bstr  hifijson  jaq-core  jaq-json  jaq-std  libm  regex-bites  self_cell  typed-arena  urlencoding
```

Several of jaq's own dependencies were already in the workspace's tree and add nothing: `jiff`
(the Client's timestamps), `base64`, `aho-corasick`, `indexmap`, `num-bigint`, `num-traits` and
`bytes`. `jaq-json`'s `self_cell` arrived with it.

## Cost

Measured commit `4fd99bf8` plus this branch's changes, on the machine the [contributor
baseline](contributor-baseline/README.md) used: Apple M2, 8 logical CPUs, 16 GiB RAM, arm64; macOS
27.0.1 (26A434). Where the baseline used it, the same prefix applied: `CARGO_BUILD_JOBS=2` and an
empty, checkout-local `target/`. The Cargo registry and toolchain were warm: **clean means compiled
artifacts are absent, not an empty download cache.**

The subject is an isolated probe crate depending on the three pinned crates and nothing else,
compiled with the workspace's profiles (`debug = "line-tables-only"`, dependencies without
debuginfo). Building the subtree alone, rather than the Client or the workspace, keeps the
measurement to jaq's own contribution and leaves the shared workspace `target/` alone. It is one
sequential sample, not a statistical benchmark.

The probe is committed at [`jaq-probe/`](jaq-probe), so the measurement can be re-run:

```sh
cd docs/measurements/jaq-probe
env CARGO_TARGET_DIR="$PWD/target" CARGO_BUILD_JOBS=2 cargo generate-lockfile
time env CARGO_TARGET_DIR="$PWD/target" CARGO_BUILD_JOBS=2 cargo build --locked
du -sk target
```

| Workload | Command after prefix | Wall time | Target |
| --- | --- | ---: | --- |
| Clean jaq subtree | `cargo build --locked` in the probe | 10.84 s | 152,552 KiB |
| The ten new crates' rlibs | `du -sk target/debug/deps/lib{jaq-core,jaq-std,jaq-json,bstr,hifijson,libm,regex-bites,self_cell,typed-arena,urlencoding}-*` | — | 17,207 KiB |

The 10.84 s covers the ten new crates and their unique dependencies, including `jiff`'s static
build. On the real workspace the same crates compile once and are then shared by the Client and no
other crate: only `kestrel-client` depends on them.

## Scope

This measures the embedded implementation's cost and nothing more. It is not a claim that
`--jq` is jq: it is jaq, a jq-compatible implementation, and the documented expressions in
[the manual](../../usage/manual-workspaces.md#filter-a-response-with-jq) are the compatibility the
Client relies on. The contributor benchmark's own workloads and numbers are unchanged.
