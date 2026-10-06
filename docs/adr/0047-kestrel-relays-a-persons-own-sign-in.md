# kestrel relays a person's own sign-in

> **Amends [ADR-0029](0029-a-claude-plan-serves-only-its-owners-own-kestrel.md).**

A subscription sign-in used to happen on the person's own machine, by a tool they had to know about,
with its output pasted into `kestrel profile set` and ended with Ctrl-D. kestrel now runs the
harness's own sign-in for them: the control plane starts a short-lived container from the
environment image, runs the sign-in command the catalogue names
([ADR-0046](0046-kestrel-ships-a-catalogue-of-how-each-harness-signs-in.md)), relays its URL and code
to whichever Client asked, relays the person's answer back, and holds what the command produces in
that person's Subscription Profile.

| Harness | Relayed command | What the person does |
| --- | --- | --- |
| Claude Code | `claude setup-token` | opens Anthropic's URL, signs in, pastes the code back |
| Codex | `codex login --device-auth` | opens OpenAI's URL and enters the code shown |

OpenCode Go has no sign-in to relay: the person signs in to the OpenCode console, which kestrel links
to, and pastes the key it issues.

**Why this stays inside ADR-0029.** The sign-in completes on Anthropic's own page, in the unmodified
binary. kestrel never sees a password, and relays a code the person would otherwise have typed into
the same binary in their own terminal. The token it then holds is the one ADR-0029 already lets the
person store in infrastructure they run, on their own Sessions only. The relay serves the
Operator alone, so it is never kestrel offering Claude.ai login to anyone else.

**Nothing in the path needs a browser near kestrel.** Each relayed step is a URL to open anywhere and
a code to carry back, so a person on a remote machine over SSH signs in from their laptop's browser
and the terminal they are already in.

## Consequences

- `claude setup-token` is an interactive terminal program, so the relay drives it through a
  pseudo-terminal. This is the riskiest part of the work and is proven before anything depends on it.
- Codex device authorization is in beta. Pasting `auth.json` stays available for when it fails.
- Pasting a token or key by hand stays available everywhere, for any sign-in method.
- One active relay belongs to one destination credential slot, so different people's Profiles and
  Organizations proceed independently. Either Client can continue it; an accepted import wins
  over its late result. Closing a Client leaves it running until its ten-minute deadline, while
  a control-plane restart interrupts it, cleans up its container and offers retry.
- Anthropic's guidance is vendor policy that changes without notice; ADR-0029's instruction to check
  it again before a release now covers the relay too.
