# Subscription profiles

A subscription profile gives a workspace access to one person's OpenCode, Codex, or Claude login. Kestrel stores credentials outside the image and checkout, supplies them only to sessions that name the profile, and records the profile's owner.

## Declare and populate a profile

For an OpenCode Go or Zen key, declare a profile and set the variable the harness reads:

```sh
kestrel profile declare jack --owner jack
kestrel profile set jack --variable OPENCODE_API_KEY
```

`profile set` reads the value from standard input; paste it and press Ctrl-D. It stores the value encrypted and does not print it again. `kestrel profile list` shows only the names of items held by each profile.

Codex and Claude can use subscription logins instead of provider keys. Supply a login file or token according to the harness:

```sh
kestrel profile set jack --file .codex/auth.json < ~/.codex/auth.json
kestrel profile set jack --file .local/share/opencode/auth.json < ~/.local/share/opencode/auth.json
kestrel profile set jack --variable CLAUDE_CODE_OAUTH_TOKEN
```

A file is placed beneath the agent's home when the session starts. At the end of the session, Kestrel reads it back and removes it from the instance, allowing a refreshed login to reach the next session. Codex writes `auth.json` when `cli_auth_credentials_store = "file"` is set where you log in. Claude Code keeps its macOS login in Keychain; run `claude setup-token` and store its output as `CLAUDE_CODE_OAUTH_TOKEN`.

OpenCode 2 imports `auth.json` into its own SQLite database once. It does not write refreshed OAuth tokens back to that file, so a rotated login must be supplied again. An `OPENCODE_API_KEY` subscription key does not have that limitation.

A Claude plan may serve only its owner's own Kestrel workspaces. Do not share that token with other people's workspaces; use a provider API key for shared work. See [ADR-0029](../adr/0029-a-claude-plan-serves-only-its-owners-own-kestrel.md).

## Use and check the profile

```sh
kestrel workspace open --project kestrel --agent codex --profile jack
```

Follow-up sessions in the workspace retain the profile. A trigger can also name one with `profile: jack`; review that declaration because the current operator API does not authenticate callers. Kestrel runs only one Codex session per profile at a time to avoid simultaneous refreshes invalidating each other. `KESTREL_SERIALIZED_HARNESS` or `--serialized-harness` controls which harnesses use that limit.

The subscription smoke tests make a real model call, restart the control plane, replace the first instance, and call again:

```sh
export KESTREL_SMOKE_CODEX_AUTH=~/.codex/auth.json
export KESTREL_SMOKE_OPENCODE_API_KEY=... KESTREL_SMOKE_OPENCODE_MODEL=opencode-go/glm-5.3
export KESTREL_SMOKE_CLAUDE_OAUTH_TOKEN=...
cargo test --locked --package kestrel --test subscription -- --ignored --test-threads 1
```

Set only the credentials for the harnesses you intend to test, and select one test such as `codex_answers` to check a single harness. The Codex test can write a refreshed login back to `KESTREL_SMOKE_CODEX_AUTH`; keep other Codex processes off that login while it runs. Failures distinguish launch, authentication, entitlement, and persistence problems.
