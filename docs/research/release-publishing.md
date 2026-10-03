# Publishing multi-arch images and CLI release binaries

Research for [#486](https://github.com/openkestrel/kestrel/issues/486), checked 3 October 2026 against Docker's
and GitHub's documentation, the source of `cargo-dist`, and the docs of `release-plz` and
`release-please`. The goal comes from spec [#460](https://github.com/openkestrel/kestrel/issues/460) ("Images and
releases"): CI publishes `ghcr.io/openkestrel/kestrel`, `kestrel-client` and `kestrel-env` for
amd64 and arm64, tagged `:main` on every merge and by version on a release; the `kestrel` CLI
ships as release binaries for macOS and Linux on both architectures, and is also in the
control-plane image.

## Where the repository is today

- **`main.yml` publishes single-arch images.** On a push to `main` or a `v*` tag it runs
  `docker build` then `docker push` for `kestrel` and `kestrel-client` only, tagged `latest` and
  the commit SHA on `main`, and the tag name on a tag. `kestrel-env` is never published under a
  stable tag; CI pushes it as `kestrel-env:<sha>` for its own jobs. Nothing sets a label.
- **CI already builds on a native arm64 runner.** The `architectures` job runs on
  `ubuntu-24.04-arm` and builds the `opencode` and `gh` stages with `--output type=cacheonly`,
  so the `TARGETARCH` branches in `images/kestrel-env/Dockerfile` are exercised.
- **Image builds use the GHA cache with per-image scopes**, written only by a trusted push to
  `main` (ADR-0027): `type=gha,mode=max,scope=kestrel|kestrel-env|kestrel-client`.
- **The Dockerfiles do not cross-compile.** `cargo build --release` runs inside the `rust:…-slim`
  stage for whatever platform BuildKit is building, so an arm64 image built under QEMU means
  `rustc` itself emulated.
- **The control-plane image carries no CLI.** `images/kestrel/Dockerfile` builds `--package kestrel`
  only; the CLI is the `kestrel` binary of `crates/kestrel-client`.
- **Both binaries link C.** `libsqlite3-sys` (control plane, ADR-0008) and `aws-lc-sys`, which
  `reqwest`'s `rustls` feature pulls into the control plane, the supervisor and the CLI
  (`cargo tree -i aws-lc-rs`). Any cross build needs a C cross compiler for the target.
- **The workspace version is `0.0.0`**, nothing is published to crates.io, and commit subjects
  are plain sentences, not Conventional Commits.
- **All three GHCR packages already exist and are public**, `kestrel` linked to the repository.
  GHCR makes a new package private on first publish
  ([GitHub docs](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)),
  so no visibility step remains.

## Native arm64 runners versus QEMU

- **Native arm64 runners are free for this repository.** "Use of the standard GitHub-hosted
  runners is free and unlimited on public repositories", and the standard set includes
  `ubuntu-24.04-arm` and `ubuntu-22.04-arm`, macOS arm64 (`macos-14`, `macos-15`, `macos-latest`)
  and macOS Intel (`macos-15-intel`)
  ([GitHub-hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
- **Docker now steers multi-platform CI away from QEMU.** Its guide shows the
  `setup-qemu-action` single-runner form, then: "Building multiple platforms on the same runner
  can significantly extend build times, particularly when dealing with complex Dockerfiles", and
  points to the Docker GitHub Builder to split platforms across runners
  ([multi-platform guide](https://docs.docker.com/build/ci/github-actions/multi-platform/)).
  A Rust release build under QEMU is the worst case of a "complex Dockerfile".
- **Cross-compiling in the Dockerfile is the other way out** (`FROM --platform=$BUILDPLATFORM`,
  `cargo build --target aarch64-unknown-linux-gnu`), but both `libsqlite3-sys` and `aws-lc-sys`
  would need an aarch64 C toolchain in the build stage, which is the cost ADR-0008 declined to
  carry for musl. ADR-0008's own consequence says arm64 waits "until either a native runner or a
  cross toolchain is worth carrying"; the native runner is now free and already in use.

**Recommendation:** build each platform natively, one runner per platform, and merge.

## Assembling one multi-arch tag

Two shapes, same result.

1. **Docker GitHub Builder** (`docker/github-builder`, v1.17.0, 21 Aug 2026), Docker's
   recommended path. A reusable workflow, `build.yml`, takes `platforms`, `push`, `file`,
   `context`, `meta-images`, `meta-tags`, `cache`, `cache-scope`, `set-meta-labels`, `sbom` and
   `sign`. With `distribute: true` (the default) it "splits the build into one platform per
   runner and assembles the final multi-platform image in its finalize phase", mapping
   `linux/arm64` to `ubuntu-24.04-arm` and everything else to `ubuntu-24.04`; `sign: auto` signs
   attestation manifests when the image is pushed. Registry credentials go in through the
   `registry-auths` secret, and the caller grants `contents: read` and `id-token: write`
   ([build.yml](https://docs.docker.com/build/ci/github-actions/github-builder/build/),
   [architecture](https://docs.docker.com/build/ci/github-actions/github-builder/architecture/)).
   For GHCR the caller also grants `packages: write`. Three images are three calls (or one
   `bake.yml` call over a Bake file with three targets).
2. **A hand-rolled matrix.** One job per platform runs `docker/build-push-action` with
   `outputs: type=image,push-by-digest=true,name-canonical=true,push=true`, uploads the digest as
   an artifact, and a merge job runs `docker buildx imagetools create -t <tag> <name>@<digest>…`,
   which creates "a new manifest list based on source manifests" already in the registry and
   accepts `--annotation "index:…"` for index annotations
   ([imagetools create](https://docs.docker.com/reference/cli/docker/buildx/imagetools/create/)).
   This is the glue the Builder now hides; it is the fallback if the Builder's inputs do not
   reach something kestrel needs, such as the `TRUSTED`-only cache writes below.

CI's per-change build jobs keep `--load` and a single platform: the local image store cannot load
a multi-platform image without the containerd image store, and attestations need a registry push
([multi-platform guide](https://docs.docker.com/build/ci/github-actions/multi-platform/),
[attestations](https://docs.docker.com/build/ci/github-actions/attestations/)). Multi-arch is a
`main.yml` concern, not a gate concern; the gate's `architectures` job stays the arm64 smoke test.

## Buildx caching

- **`type=gha` stays.** Scope must be set per image so builds do not overwrite each other
  ([gha backend](https://docs.docker.com/build/cache/backends/gha/)). With two platforms per
  image the scope also has to differ per platform (`kestrel-amd64`, `kestrel-arm64`), or the two
  runners' `mode=max` exports clobber each other; check whether the Builder's `cache-scope`
  suffixes the platform before relying on it.
- **The budget is 10 GB per repository**, evicted by last access, and entries unused for 7 days
  are removed ([dependency caching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)).
  Doubling image scopes plus `rust-dependencies` and `bun` will press on it; `mode=max` of a
  Rust build stage is the large item. If it thrashes, `type=registry,ref=…:buildcache` moves the
  image cache to GHCR, where it has no such cap
  ([cache guide](https://docs.docker.com/build/ci/github-actions/cache/)).
- **A tag run reads `main`'s cache but writes a cache no later run can read.** "Workflow runs can
  restore caches created in either the current branch or the default branch", but "cannot
  restore caches created for different tag names" (same page). So a release workflow restores
  from `main` and never writes, which ADR-0027's trusted-writer rule already implies.
- `RUN --mount=type=cache` contents are not kept in the GHA cache without
  `reproducible-containers/buildkit-cache-dance`
  ([cache guide](https://docs.docker.com/build/ci/github-actions/cache/)); the Dockerfiles use no
  cache mounts, so this does not bite today.

## Tags and provenance labels

- **`docker/metadata-action`** (v6.2.0) computes tags and labels from the event. `type=raw,value=main`
  (or `type=ref,event=branch`) gives `:main`; `type=semver,pattern={{version}}` gives `:0.4.0`
  from tag `v0.4.0`, and with the default `flavor: latest=auto` a semver tag also moves `latest`.
  It emits the OCI labels `org.opencontainers.image.title`, `description`, `url`, `source`,
  `version`, `created`, `revision` and `licenses`, and an `annotations` output whose levels are
  set by `DOCKER_METADATA_ANNOTATIONS_LEVELS` (default `manifest`; `manifest,index` for a
  multi-arch tag) ([metadata-action](https://github.com/docker/metadata-action)).
- **GHCR reads three of them.** `org.opencontainers.image.source` connects the package to the
  repository; `description` (512 characters) and `licenses` (SPDX, 256 characters) are shown on
  the package page. "For images supporting multiple architectures", the description has to be an
  annotation on the index, not a label
  ([GHCR docs](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)).
- **`dev.kestrel.harnesses`** (spec #460) is a Dockerfile `LABEL`, not a metadata-action label: it
  describes the image's contents, and the Docker driver reads it from `GET /images/{name}/json`,
  which reports the platform manifest's config labels, not index annotations.
- **Build provenance.** `build-push-action` attaches a SLSA provenance attestation by default,
  `mode=max` on a public repository, and none with `load: true`; SBOMs need `sbom: true`
  ([attestations](https://docs.docker.com/build/ci/github-actions/attestations/)). The Builder
  additionally signs them. For GitHub's own attestation store, `actions/attest` (which
  `attest-build-provenance` v4 now wraps) signs a subject by `subject-name` + `subject-digest`
  for an image or `subject-path` for binaries, needs `id-token: write` and `attestations: write`,
  is free on public repositories, and is checked with `gh attestation verify`
  ([attest-build-provenance](https://github.com/actions/attest-build-provenance)). Provenance
  embeds build-arg values, which is harmless here because no secret is a build arg.

## CLI release binaries

**`cargo-dist` (`dist`)**, v0.33.0, 10 Sep 2026, actively maintained.

- `dist init` generates `release.yml`, which runs plan → build → host → publish → announce on a
  pushed version tag (`v1.0.0`, or `<package>-v1.0.0` per package) and uploads archives,
  checksums and installers (shell, PowerShell, Homebrew, npm, MSI) to a GitHub Release
  ([book](https://axodotdev.github.io/cargo-dist/book/)).
- **Its default runners are already native for every kestrel target**
  ([`github_runner_for_target`](https://github.com/axodotdev/cargo-dist/blob/main/cargo-dist/src/backend/ci/github.rs)):
  `aarch64-unknown-linux-gnu` → `ubuntu-22.04-arm`, `x86_64-unknown-linux-gnu` → `ubuntu-22.04`,
  `aarch64-apple-darwin` → `macos-14`, `x86_64-apple-darwin` → `macos-15-intel`. Building on 22.04
  "to minimize the places where random system dependencies can creep in" sets the CLI's glibc
  floor at 2.35. `github-custom-runners` overrides any of these
  ([config](https://axodotdev.github.io/cargo-dist/book/reference/config.html)).
- **musl is one target away.** For `*-linux-musl` targets dist installs `musl-tools` and builds on
  the matching native runner, which provides the C compiler `aws-lc-sys` needs. ADR-0008 is about
  the control plane; a static CLI is not covered by it, and would run on Alpine. Untested here.
- **Scoping to the CLI.** Set `dist = false` on `kestrel`, `kestrel-supervisor` and
  `kestrel-scripted-agent`, or list `packages`, so only `kestrel-client`'s `kestrel` binary is
  shipped; `github-attestations = true` attests each archive (marked experimental); `pr-run-mode`
  defaults to `plan`, adding a cheap check to pull requests that can be set to `skip` given
  ADR-0027's single required `gate`
  ([config](https://axodotdev.github.io/cargo-dist/book/reference/config.html)).
- **It does not bump versions or write changelogs.** If a `CHANGELOG.md`/`RELEASES.md` exists it
  copies the section whose heading matches the version into the GitHub Release
  ([simple guide](https://github.com/axodotdev/cargo-dist/blob/main/book/src/workspaces/simple-guide.md)).
- dist owns `release.yml` and regenerates it (`dist generate`); hand edits are flagged unless
  listed in `allow-dirty`, which its docs discourage. Image publishing therefore belongs in its own
  workflow on the same tag, not in dist's file, or in a `publish-jobs` custom job it calls.

The hand-rolled alternative is a four-entry matrix (`ubuntu-24.04`, `ubuntu-24.04-arm`,
`macos-15`, `macos-15-intel`) running `cargo build --release -p kestrel-client`, `tar` +
`sha256sum`, `actions/attest`, and `gh release upload`. It is about the size of one of dist's
jobs, and gives up installers, the PR plan check and `dist-manifest.json`.

## Version bump and changelog

| | release-plz | release-please | neither |
| --- | --- | --- | --- |
| What it does | A release PR bumping `Cargo.toml` and `CHANGELOG.md` (git-cliff); on merge, tags and creates the GitHub Release | Same, language-agnostic; a Cargo workspace needs manifest config and the `cargo-workspace` plugin | Bump the workspace version and the `compose.yaml` pins in an ordinary PR, tag the merge |
| Commit convention | "assumes" Conventional Commits; others are a patch bump | "assumes" Conventional Commits | none |
| Without crates.io | `publish = false`, `git_only = true`, `git_tag_name = "v{{ version }}"` | supported | n/a |
| Triggers the tag workflow? | Not with `GITHUB_TOKEN`; needs a PAT or GitHub App | Same | Yes, a person pushes the tag |
| Version | v0.3.169, 19 Sep 2026 | v17.11.2, 24 Aug 2026 | |

Sources: release-plz [config](https://release-plz.dev/docs/config),
[changelog format](https://release-plz.dev/docs/changelog/format),
[token](https://release-plz.dev/docs/github/token); [release-please](https://github.com/googleapis/release-please);
"events triggered by the `GITHUB_TOKEN` will not create a new workflow run"
([GITHUB_TOKEN](https://docs.github.com/en/actions/concepts/security/github_token)).

Both tools derive the next version and the changelog from Conventional Commit prefixes. Kestrel's
subjects ("Close open units when a Session loses its supervisor (#469)") would all read as patch
bumps under an "Other" heading, so either tool means adopting a commit convention first, plus a
bot token. Neither publishes anything kestrel ships. GitHub's generated release notes (a list of
merged PRs, grouped by label through `.github/release.yml`) fit sentence-style PR titles as they
are ([GitHub docs](https://docs.github.com/en/repositories/releasing-projects-on-github/automatically-generated-release-notes)).

## Fit for kestrel

1. **Release PR, by hand.** Bump `[workspace.package] version`, pin `compose.yaml`'s three images
   to that version, and merge. Then push `v<version>` on the merge commit. dist picks what to
   release by matching the tag against package versions, so a tag that matches no version
   releases nothing.
2. **Binaries: `cargo-dist`.** `dist init` with the four `*-apple-darwin` / `*-unknown-linux-gnu`
   targets (or `-musl` for Linux), the CLI as the only distributed package, the shell installer,
   `github-attestations = true`, `pr-run-mode = "skip"`. Its default runners are native on every
   target.
3. **Images: on `main` and on `v*`, one native runner per platform**, through
   `docker/github-builder` or the push-by-digest matrix, with `metadata-action` giving `:main` (and
   the SHA) on `main` and `:<version>` plus `latest` on a tag, `org.opencontainers.image.*`
   labels and an index-level description. This replaces `main.yml`'s `docker build`/`docker push`
   and starts publishing `kestrel-env`. Spec #460 wants `:main` where `main.yml` writes `latest`
   today; with `latest=auto`, `latest` comes to mean the newest release.
4. **Caching:** keep `type=gha`, scoped per image and platform, written only from `main`; watch the
   10 GB cap and move to `type=registry` if eviction shows up in build times.
5. **Provenance:** keep BuildKit's default `mode=max` provenance (signed if the Builder is used),
   and add `actions/attest` for the image digests and the release archives if `gh attestation
   verify` is something an operator should be able to run.
6. **Owed outside CI:** the control-plane Dockerfile must also build `--package kestrel-client` and
   copy `kestrel` into the image, and ADR-0008's "The published image is `linux/amd64`"
   consequence is superseded by whichever ADR records native multi-arch publishing.
