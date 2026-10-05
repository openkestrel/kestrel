# A tag prepares a release; acceptance publishes it

A pushed version tag currently publishes images while Conformance runs independently. The `0.4`
acceptance journeys need tagged artifacts, but their existence cannot also mean the rung passed.
A reviewed release PR sets one product version and a curated changelog; the maintainer tags its
merged commit. Automated release checks, including Conformance, pass before artifacts are published
into a draft release. Fresh-machine acceptance then authorizes the maintainer to publish the release
and move the ROADMAP marker. The decision is recorded in
[Decide how a kestrel release is cut](https://github.com/openkestrel/kestrel/issues/496).

The product version belongs to the Rust workspace and is recorded in the browser build, without
independently versioning private npm packages. Release notes describe behavior changes, fixes and
known limitations. The release publishes `kestrel`, `kestrel-client`, `kestrel-env` and `kestrel-dev`
images for Linux amd64 and arm64, built on native runners and joined into multiarch manifests.
The `kestrel` CLI ships for Linux and macOS on both architectures through cargo-dist. The
control-plane image also carries the CLI. Windows distribution does not belong to this rung.

Development `compose.yaml` keeps its source builds on main. A pull-only `compose.yaml` is generated
from the tagged source after the image manifests exist and attached to the draft release. It pins
the three product images by multiarch manifest digest, including the control plane's Instance image
reference; `kestrel-dev` remains a separately published contributor image. Main publishes `main` and
commit-SHA image tags. `latest` moves only to an accepted release, consistently across all four
images. Published version tags and artifacts are immutable: failed acceptance leaves the release
in draft, and a reviewed patch release supplies the next candidate. The `0.4` gate accepts the
successful tagged `0.4.x` release rather than requiring replacement of a failed `v0.4.0`.

This supersedes only ADR-0008's amd64-only publication consequence; its dynamic-linking decision
stands. Native builds buy coverage of both host architectures without carrying an emulation-based
release build pipeline. Separating artifact preparation from release publication keeps acceptance
honest while giving it the exact artifacts people will install.
