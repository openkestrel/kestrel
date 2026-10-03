# kestrel ships a catalogue of how each harness signs in

> **Amends [ADR-0025](0025-subscription-profiles-are-personal.md)**, which held that kestrel "holds no
> table of where each runtime keeps its login: the person names the path."

A person setting kestrel up should choose a harness and sign in to it, not learn that Claude Code
reads `CLAUDE_CODE_OAUTH_TOKEN`, that OpenCode cannot use that token as `ANTHROPIC_API_KEY`, or that
Codex keeps its login in `.codex/auth.json`. Every one of those was a failure in the first run that
prompted this, and each ended in an error that named none of it. So the control plane carries a
**catalogue**: for each harness it supports, its command and its **Sign-in Methods**, each one a
subscription or a key, and the variable or file that method fills.

| Harness | Subscription | Key |
| --- | --- | --- |
| Claude Code | `claude setup-token` → `CLAUDE_CODE_OAUTH_TOKEN` | `ANTHROPIC_API_KEY` |
| Codex | `codex login --device-auth` → `.codex/auth.json` | `OPENAI_API_KEY` |
| OpenCode | OpenCode Go or Zen console key → `OPENCODE_API_KEY` | `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` |

**The kind of method decides where it is held, and nobody is asked.** A subscription is personal
(ADR-0025) and a Claude plan cannot be shared (ADR-0029), so a subscription is always held in its
signer's own Subscription Profile and a key always as the Organization's Provider Credential.

**The catalogue is data, and Profiles stay generic.** It names what the existing entries are for; a
Profile still holds any variable or file a person names, and a harness outside the catalogue still
runs from the control plane's harness-command setting. kestrel simply cannot guide or vouch for one.

## Considered options

- **Keep the person naming paths, and document them better.** The documents existed; the first run
  still failed three ways.
- **Ask each harness over ACP.** ACP's authentication methods name ways to log in, not what a
  process must be given to start already logged in, which is the question a Profile answers.

## Consequences

- A harness release that moves its login changes a catalogue row, not kestrel's code.
- OpenCode's own OAuth providers (`opencode auth login`) stay out: OpenCode 2 imports an `auth.json`
  once and refreshes only its own database (ADR-0026, #369), so kestrel could not keep such a login
  alive.
- Each sign-in is checked with one real model call when it is saved, and carries a state a reader can
  act on: signed in, expired, or not covered by the plan.
