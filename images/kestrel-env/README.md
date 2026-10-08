# `kestrel-env`

The base image a **Session** executes in. Its Dockerfile is here so the image is derivable rather than
opaque ([ADR-0002](../../docs/adr/0002-two-deployables-the-environment-dials-out.md)).

It carries five things: the **supervisor**, **opencode**, **git**, **Node** with the **Claude Code
and Codex ACP adapters**, and the **`claude` and `codex` vendor executables** the subscription
sign-in relays run. Every harness in the catalogue runs here, and the image says so in its
`dev.kestrel.harnesses` label, which a derived image inherits and overrides when it carries a
different set ([ADR-0048](../../docs/adr/0048-the-environment-image-carries-every-catalogued-harness.md)).
A **Project**'s setup layers its repositories' dependencies on top; injecting the
supervisor into a bring-your-own image is the `0.8` escape hatch, and installing an agent from the
ACP Registry is `0.8` work under the constraints ADR-0007 records. Nothing else here downloads an
agent.

## Building it

The build context is the repository root, because the supervisor is built from source in it:

```sh
docker build --file images/kestrel-env/Dockerfile --tag kestrel-env .
```

Almost everything the build pulls in is pinned: both base images by digest, the Rust toolchain by
`rust-toolchain.toml`, the crates by `Cargo.lock` under `--locked`, the adapters and vendor
executables by `package-lock.json` under `npm ci`, and opencode by version and
SHA-256. The exception is apt, which resolves `git` and `ca-certificates` to whatever the Debian
release carries on the day — the one thing here that moves without the Dockerfile changing.

opencode 2 is a standalone build from
`https://opencode.ai/files/bin/<version>/opencode-linux-<arch>[-baseline].tar.gz`, not a GitHub
release. That channel publishes no checksums file, so a bump reads the SHA-256 from upstream's
update API, which maps each artifact to its URL and digest:

```sh
curl --location --silent "https://opencode.ai/update/api/latest/cli/opencode"
```

Take `.version` for `OPENCODE_VERSION`, and `.metadata.files["opencode-linux-x64-baseline.tar.gz"].sha256`
and `.metadata.files["opencode-linux-arm64.tar.gz"].sha256` for `OPENCODE_SHA256_AMD64` and
`OPENCODE_SHA256_ARM64`. x64 takes the `-baseline` artifact, which does not require AVX2, so a host
without it still runs; arm64 has no baseline variant and takes the default. Both are glibc builds,
matching the Debian base. The documentation page lags the files site, so read the version from the
update API rather than from it.

CI writes the image's size and build time into every run's summary, so growth is visible in the run
that causes it. The opencode binary is nearly all of the size.

## Running one by hand

The Instance's supervisor is the image's entrypoint, and it needs the control plane's address, the
Instance it runs on, and that Instance's link credential.

```sh
docker run --rm \
  --add-host host.docker.internal:host-gateway \
  --env KESTREL_LINK=http://host.docker.internal:7717 \
  --env KESTREL_INSTANCE=<driver>/<instance> \
  --env KESTREL_INSTANCE_CREDENTIAL=<credential> \
  kestrel-env
```

Each Session is begun over the link: its `start` names the harness command, the model the Session's
Agent named (set over ACP once the ACP session is open), and the ACP authentication method, if any.
The supervisor stays on the link between Sessions.

**No provider key is among them.** The **Provider Credentials** the Session's Organization holds arrive
over the link as the supervisor spawns the agent, and reach that process's environment and nothing
else ([ADR-0010](../../docs/adr/0010-a-provider-credential-crosses-the-link-at-the-spawn.md)). An
Environment run by hand this way reaches whatever the operator's own shell put in it, which is the
one thing the shipped path does not do.

The supervisor dials the link outward and the image exposes no port: an Environment needs egress and
nothing else, which is the capability every deployment target has.

It runs as the unprivileged `kestrel` user in `/workspace`, where a Project's repositories are
checked out side by side. An ACP session's working directory is the checkout of the first of them. Anything written into an Environment from
outside has to land owned by that user, and a derived image that installs packages needs `USER root`
first.

**Killing the supervisor ends the Session it carries.** The agent is the supervisor's child over
stdio, so nothing in the image restarts one — the entrypoint is the supervisor itself, with no init or
wrapper around it. A supervisor that dies takes its Session with it: the lease expires unheld, and the
Session ends failed with an explicit exit status. That is the trade ADR-0007 records, and `crates/kestrel/tests/image.rs`
holds it to it.

## Deriving from it

A derived image adds what one kind of work needs and keeps the supervisor as its entrypoint.
[`images/kestrel-dev`](../kestrel-dev/README.md) is the one this repository ships. It adds Rust,
`gh`, and nothing else: the adapters and vendor executables it once installed live here now, and
it inherits them along with the harness label. An adapter usually
advertises more than one way to sign in, and ACP gives a client no way to choose between them, so an
Agent on a derived image is configured with `--agent-auth` as well.
`crates/kestrel/tests/support/conformance-env.Dockerfile` is a smaller worked example. It builds the
conformance suite's second agent this way and drives it without any Kestrel code branching on it.

## Bumping an adapter or a vendor executable

Change its exact version in `package.json`, then regenerate the lockfile in the same Node image
the build uses so the Linux optional dependencies are recorded:

```sh
docker run --rm --volume "$PWD/images/kestrel-env:/w" --workdir /w \
  node@sha256:50c3b2f6988dfc307b86e5301d69611af31f4789bdf232863b07d3b02fe55ae0 \
  npm install --package-lock-only --ignore-scripts
```

`claude` is a native binary its wrapper's postinstall copies over a stub, and the build keeps
`--ignore-scripts` for everything else: the Dockerfile runs that one install step explicitly, so a
release that stops shipping a Linux binary fails the build instead of shipping a stub.
