# The environment image carries every catalogued harness, and says so

> **Amends [ADR-0007](0007-acp-is-the-agent-runtime-contract.md)**, which left Node out of
> `kestrel-env`, and **[ADR-0009](0009-the-daemon-is-reached-through-a-filtered-proxy.md)**, by one
> read.

`kestrel-env` carried opencode alone, and the Claude Code and Codex adapters lived in `kestrel-dev`,
which Compose never builds. kestrel nonetheless accepted `--harness claude`, and the Session died
with `env: 'claude-agent-acp': No such file or directory`. Offering a harness the image cannot run
is the defect, so the default image carries every harness in the catalogue
([ADR-0046](0046-kestrel-ships-a-catalogue-of-how-each-harness-signs-in.md)): opencode,
`claude-agent-acp` and `codex-acp`, and the Node the two adapters need. `kestrel-dev` keeps only what
working on kestrel itself adds: Rust and `gh`.

**An image declares the harnesses it carries in a label**, and kestrel reads the label from the
image's metadata without running it. A derived image inherits the label and overrides it when it
adds one. That answers "can this image run Claude Code?" before any Instance exists, which is when a
first run asks it, and it works on any compute backend, since every one pulls an OCI image. A test
holds the label honest by checking that each harness it lists is on the image's `PATH`.

**The Docker driver gains one read**, `GET /images/{name}/json`, in the socket proxy's filter.

## Considered options

- **Choose an image per harness.** Smaller images, at the cost of a concept a first run would have to
  explain, for a few hundred megabytes.
- **Probe the image at startup** by running each harness's command. kestrel guessing about an image
  from outside, at a cost on every start.
- **Have the Supervisor report what it can spawn** when it dials the link. The most truthful source,
  but silent until an Instance exists, so a first run could not say whether a harness is available
  until it had opened a Workspace to find out.

## Consequences

- The default image is larger, and the image build installs the adapters with `npm ci` from a
  committed lockfile.
- Declaring an Agent whose harness the image does not declare is refused at once, naming the image.
- A Session that still meets a missing command, on an image whose label lies, fails naming the image
  and the command rather than with the shell's `env:` line.
