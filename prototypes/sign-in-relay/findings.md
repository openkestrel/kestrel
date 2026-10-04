# Sign-in relay findings

Measured 2026-10-04, on Linux arm64 in Docker 29.2.1, using
`kestrel-dev-accept03`. Claude Code 2.1.274 is the native executable bundled with
Claude Agent SDK 0.3.274; Codex CLI is 0.156.1. These are the versions pinned in
`images/kestrel-dev/package-lock.json`, rather than the host's newer CLIs.

## Image prerequisite

`kestrel-env` currently ships only OpenCode. The development image has Codex at
`/opt/acp/node_modules/.bin/codex`, without a global `codex` command, and Claude at
`/opt/acp/node_modules/@anthropic-ai/claude-agent-sdk-linux-arm64/claude`, without
a global `claude` command. The SDK executable accepts `setup-token`; the image
work must expose both executable commands as well as their ACP adapters. Moving
the npm lockfile alone does not put either sign-in command on PATH.

## Claude framing and failures

A fresh container enters `setup-token` directly, without a theme, trust or account
selection question. The screen describes a one-year token requiring a Claude
subscription, attempts to open a browser, then shows the complete authorization
URL and `Paste code here if prompted >`.

The authorization URL uses a PKCE challenge and a per-attempt state. It currently
starts at `https://claude.com/cai/oauth/authorize`, with a code callback at
`https://platform.claude.com/oauth/code/callback` and the inference scope. No port
was published to the host; a pasted code is the route being tested.

This is terminal-screen output, not a line protocol. It uses horizontal cursor
movement instead of some spaces, redraws and a spinner, and wraps the URL even at
240 columns. Stripping ANSI alone destroys word boundaries. Feed bytes through
an incremental terminal decoder and reconstruct the URL from adjacent screen
rows. Do not depend on chunks or rendered lines being complete.

Both 80 and 240 columns, with 40 rows, reach the prompt. `TERM=dumb` still emits
terminal controls and reaches the prompt. Claude configures noncanonical input,
no echo, and no ISIG; send carriage return to submit. The host PTY starts in its
default mode; the command itself sets raw mode. The Docker console's `stty -a`
confirms the mode. With pipes and TERM=dumb, no bytes arrived within ten seconds;
that is an observed stall, not a proof that every pipe configuration fails.

A deliberately malformed pasted code yields:

```text
OAuth error: Invalid code. Please make sure the full code was copied
Press Enter to retry.
```

The process stays alive. An exit-only parser would leave the relay waiting.

## Codex framing and failures

The device-auth prompt is regular text, with a fixed verification URL,
`https://auth.openai.com/codex/device`, and a one-time device code. It says the
code expires in fifteen minutes. The person enters that code in the browser;
there is no pasted answer for stdin. Both PTY mode and pipe mode with TERM=dumb
produce the prompt and wait for browser authorization. A PTY is optional here.

Force `-c cli_auth_credentials_store="file"` and use an empty home so that a
successful login is recoverable at `/home/kestrel/.codex/auth.json` without
accidentally consulting a different credential store.

The pinned upstream implementation requests `/api/accounts/deviceauth/usercode`,
polls `/api/accounts/deviceauth/token`, exchanges the returned authorization code,
checks any forced workspace, then persists tokens. It treats polling 403 and 404
as pending, enforcing a fifteen-minute deadline on those responses; other HTTP
errors fail. A disabled device-code endpoint, network/TLS error, token exchange
error, forbidden workspace or credential persistence error can prevent success.
These paths were read in source, not all induced in this probe.

kestrel's specified ten-minute deadline is shorter than Codex's fifteen minutes;
it must terminate the container itself. A hung HTTP request also needs an external
deadline. Every timeout probe here removed its own container successfully.

## Live credential capture

Codex completed with the person entering the device code in their browser. The
command exited naturally with status 0 after 133 seconds. The stopped container
held a JSON auth file with `auth_mode: chatgpt` and nonempty `access_token`,
`refresh_token`, `id_token` and `account_id` fields. Only field names and presence
were recorded; the credential and container were removed.

Claude also completed with the person returning the browser's code to the
probe. The PTY decoder detected the `sk-ant-oat` token in the rendered output;
the command exited naturally with status 0 after 430 seconds including the
person's browser interaction. The driver submitted the code with carriage return
and subsequently sent another Enter. Only token presence was recorded, not its
value. The container was removed. This proves a successful real sign-in and
detectable token output, not an inference call with that token.

These are one successful live run per pinned tool on arm64. Repetition across
versions and amd64 belongs in the ignored vendor smoke tests, not deterministic
CI tests.

## Consequences for the plan

Both pinned sign-in commands completed in fresh, short-lived containers with no
published ports. ADR-0047 needs no fallback-only retreat on this evidence. The
parsing and
interaction belong to per-tool adapters. Claude needs terminal decoding and an
explicit failed/retry transition and continuation input if it does not exit after
showing its token. Codex polls and accepts no code on stdin. A
relay should never publish raw terminal output: it can contain a token.

Pin the versions tested, pass only URL/code/state to a Client, intercept credential
output privately, and validate a natural successful command exit plus a captured
credential before saving. Preserve manual token/auth-file entry when device auth
is unavailable. Real vendor sign-ins remain ignored smoke tests, with scripted
boundary tests for deterministic relay states.

## Primary references

- [Claude authentication](https://code.claude.com/docs/en/authentication): browser-returned code fallback and restrictions.
- [Codex authentication](https://learn.chatgpt.com/docs/auth): device-code enablement, beta status, and file/keyring storage.
- [Codex device-auth source, pinned rust-v0.156.1](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/login/src/device_code_auth.rs): polling, expiry, exchange and persistence.
