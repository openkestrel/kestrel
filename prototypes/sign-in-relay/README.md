# Sign-in relay spike (throwaway)

Question: can a short-lived container expose a harness's sign-in URL, accept the
person's answer and recover the resulting credential?

This is a probe, not the implementation. It runs a new container with no mounted
home or credentials, decodes its terminal output, then removes the container.
Timeout termination is reported separately from a command's natural exit.

Run from the repository root, with Docker running and the development image built:

```sh
python3 -m venv /tmp/sign-in-relay-venv
/tmp/sign-in-relay-venv/bin/pip install -r prototypes/sign-in-relay/requirements.txt
/tmp/sign-in-relay-venv/bin/python prototypes/sign-in-relay/probe.py claude --invalid-code
/tmp/sign-in-relay-venv/bin/python prototypes/sign-in-relay/probe.py codex --pipe
```

The default image name is the local acceptance-run image, `kestrel-dev-accept03`.
Use `--image kestrel-dev` for a newly built development image. The Claude binary
path currently selects the arm64 SDK package; adapt it to the x64 package for amd64.
Both packages are pinned by `images/kestrel-dev/package-lock.json`.

`--cols 80 --term dumb` tests narrow terminal rendering. `--pipe` removes the PTY.
`--seconds N` sets a hard probe deadline. `--live --seconds 600` prints the URL and,
for Codex, the device code; type Claude's browser-returned code followed by Enter.
Codex requires no input to this process. Live runs suppress terminal snapshots and
report only whether Claude's token was seen, or Codex's auth file had credential
fields. Neither credential is kept after cleanup. The displayed login codes and
URLs are transient; do not copy them into committed evidence.

The driver intentionally omits production parsing, API transport and persistence.
Its terminal screen is 40 rows high. A production adapter needs bounded history,
streaming UTF-8 and escape decoding, and a separate parser for each pinned tool.

See [findings](findings.md) and the redacted [probe results](results/).
