# Subscription profiles

A subscription profile gives a workspace access to one person's OpenCode, Codex, or Claude login. Kestrel stores credentials outside the image and checkout, supplies them only to sessions that name the profile, and records the profile's owner.

## Sign in with `kestrel login`

`kestrel login HARNESS` signs in to a harness the catalogue knows (`claude`, `codex`, `opencode`)
and saves the result where its Sign-in Method says, without asking:

| Kind | Held in | Methods |
| --- | --- | --- |
| Subscription | Your own Profile, declared under the Operator's name if you have none | `claude-setup-token`, `codex-device-auth`, `opencode-go-zen` |
| API key | The Organization's Provider Credential, shared by its Sessions | `anthropic-api-key`, `openai-api-key` |

A subscription is your own; a Claude plan serves only its owner's own Kestrel work
([ADR-0029](../adr/0029-a-claude-plan-serves-only-its-owners-own-kestrel.md)). Use an API key for
work other people share. Name the Operator first (`kestrel operator name NAME`).

At a terminal, omitting `--method` lists every method with where it is held, and asks. Without a
terminal on both standard input and standard error, name `--method` unless the harness and
`--token` or `--file` leave exactly one: `kestrel login codex --file auth.json` is the Codex
subscription, `kestrel login codex --token` its OpenAI key.

```sh
kestrel login claude                                    # asks which method
kestrel login claude --method claude-setup-token        # relay: link, then the code, at a terminal
kestrel login codex --method codex-device-auth          # relay: link and device code, then waits
kestrel login opencode --method opencode-go-zen         # console link, then the key, unechoed
kestrel login codex --file ~/.codex/auth.json           # import a login file
printf %s "$ANTHROPIC_API_KEY" | kestrel login claude --method anthropic-api-key --token
```

**Relays.** A subscription with a relay signs in through the vendor's own command, run in a fresh
Instance by the control plane. Kestrel prints the vendor's link on a line of its own; open it in a
browser on any machine, so signing in over SSH needs no tunnel. Claude then shows a code to paste
back, read unechoed, so its relay needs a terminal. Codex shows a one-time code to enter on that
page and waits for approval, so it runs without one. A relay lasts ten minutes. If the Client loses
the control plane meanwhile, it reads the same relay again rather than starting another.

**Imports.** `--token` reads an existing token or key unechoed at a terminal, otherwise from
standard input to its end, dropping one trailing newline. `--file PATH` reads a login file from
PATH exactly as written. Neither takes the secret as an argument. A method's input must match:
a Codex login file is never `--token`, and an input that does not fit is refused before anything
is sent.

**What saving checks.** An Anthropic or OpenAI key is checked with one model-list read, which
spends no model usage; a rejected key is refused and what was saved before stays. Subscriptions
and OpenCode keys have no such check, so they are saved `unchecked`. A finished relay records
`login_completed`. If the provider cannot answer, the key is saved unchecked and `login` exits 5,
naming the revision it saved. Saving never sends a prompt.

Progress, prompts and failures go to standard error; the saved sign-in goes to standard output,
or `--json` writes it whole. Material is never shown.

## Test a saved sign-in

`kestrel sign-in test HARNESS --method ID [--model MODEL]` sends one prompt with the saved
material and so spends model usage; it says so before it starts. OpenCode tests need
`--model PROVIDER/MODEL`, the provider `opencode` or `opencode-go`; other methods test the
harness's default model without one.

```sh
kestrel sign-in test claude --method claude-setup-token
kestrel sign-in test opencode --method opencode-go-zen --model opencode-go/glm-5.3
```

It waits for the test to end and prints it, with the model use it established for that revision,
harness and model only; a key the provider accepted says nothing about which models work. A test
that fails exits 4 when the material was refused or did not cover the model, and 5 when no
Instance could run it. A login already in use by a Session, or several Profiles to choose from,
is refused before anything runs. Nothing runs a test again by itself: if the Client loses the
control plane, it reads the same test again.

## Generic input and checked imports

`profile set` accepts any variable or UTF-8 file the harness needs. It creates an absent
Profile under the stable Operator identity; name the Operator first:

```sh
kestrel operator name Jack
kestrel profile set jack --variable OPENCODE_API_KEY
```

An existing Profile keeps its ownership. `--owner NAME` selects a free-text owner only when
creating a Profile, even when NAME matches the Operator's name. Such a Profile is never
selected as the Operator's default. For example:

```sh
kestrel profile set other --owner Alex --variable OPENCODE_API_KEY
```

Choose exactly one of `--variable NAME` and `--file PATH`. Neither accepts a secret-valued
argument. At a terminal, input is hidden; paste it and press Ctrl-D. A pipe is read to EOF.
Variable input loses exactly one trailing LF or CRLF; all other whitespace stays. File input
is preserved exactly, including its trailing newline, and must be nonempty UTF-8.

Input is collected before creation. If creation succeeds but saving the material fails, the
failure says the Profile was already declared; inspect it with `kestrel profile list`.
Existing material survives rejected input. `--organization` and `--control-plane` select the
scope and connection as on other commands. Successful output lists only material metadata.

Generic writes store encrypted material, advance its revision and replace obsolete authentication
and model-use evidence with unchecked evidence. They cancel an active relay for the same slot
and make no provider or model call. `kestrel login` instead chooses the catalogue's destination,
validates its input and performs supported provider checks without model usage (see
[saved sign-ins](../architecture/operator-sign-ins.md#saved-sign-ins)). `profile set` is the manual
path for material the catalogue does not describe.

`kestrel profile declare NAME` can also declare a Profile without storing material; its omitted
`--owner` selects the Operator. `kestrel profile list` shows ownership and the names of held items.

By hand, without the catalogue, supply a login file or token according to the harness:

```sh
kestrel profile set jack --file .codex/auth.json < ~/.codex/auth.json
kestrel profile set jack --variable CLAUDE_CODE_OAUTH_TOKEN
```

A file is placed beneath the agent's home when the session starts. At the end of the session, Kestrel reads it back and removes it from the instance, allowing a refreshed login to reach the next session. Codex writes `auth.json` when `cli_auth_credentials_store = "file"` is set where you log in. Claude Code keeps its macOS login in Keychain; run `claude setup-token` and store its output as `CLAUDE_CODE_OAUTH_TOKEN`.

OpenCode signs in with `OPENCODE_API_KEY` only. Do not hold an `auth.json` as a Profile file: OpenCode 2 never imports it into a fresh home, so the file gives the harness no provider and a Session fails at start with "this agent does not offer the model …" ([ADR-0026](../adr/0026-kestrel-carries-named-credentials-never-a-runtimes-store.md) records why).

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
