# CLI first-run transcripts — throwaway prototype

Review artifact for [Prototype the CLI's first-run surface](https://github.com/openkestrel/kestrel/issues/493),
on `prototype/cli-first-run`. These are proposed outputs, not recordings of implemented commands.
Read this file directly; no build or service is needed.

## Reading the transcripts

`$` is the person's command, `out>` is stdout, `err>` is stderr, `in>` is typed input,
and `exit>` is the process exit status. Prefixes and bracketed annotations are not emitted.
Every unlisted stream is empty. Hidden input never echoes, including pasted relay codes.
Fixture URLs and device codes represent values supplied by the relay; kestrel does not
construct vendor authorization URLs. Workspace and Session references below are fixtures.

TTY styling: headings and `Next:` are bold; ready/saved is green, warning/not ready is
yellow, errors are red, inferred values are dim, URLs and commands remain plain and copyable.
No meaning depends on colour. Pipes and `NO_COLOR` have no ANSI escapes. Prompts and
progress go to stderr; successful records go to stdout. Readiness is a successful read,
so its summary goes to stdout even when the CLI exits 78; corrective diagnostics go to stderr.
Prompts require both stdin and stderr to be terminals. No spinner or terminal-control
sequences in redirected output. `--json` suppresses human progress and prompts.

The settled refusal contract owns exit codes: 0 success, 1 local failure, 2 invalid CLI
input, 3 unresolved reference, 4 rejected operation, 5 unavailable, 78 blocked setup.
A user's Ctrl-C exits 130. Reading a failed Session succeeds with 0.

## Bare kestrel: readiness and every ordered gap

### Ready

```text
$ kestrel
out> Ready · Organization Jack · Claude Code · Profile jack
out> Client: http://localhost:7719
out> Next: kestrel start
exit> 0
```

### No Operator

```text
$ kestrel
out> Not ready: name the Operator.
out> Client: http://localhost:7719
err> Next: kestrel start --operator <name>
exit> 78
```

### No usable signed-in harness

```text
$ kestrel
out> Not ready: no harness is signed in.
out> Client: http://localhost:7719
err> Next: kestrel start
exit> 78
```

`start` collects the harness choice; the repair does not arbitrarily choose Claude.

### Selected sign-in needs attention, with authentication evidence only

```text
$ kestrel --organization Jack
out> Not ready: Claude Code's sign-in in Profile jack needs attention.
out> Evidence: the harness reported authentication required.
out> Client: http://localhost:7719
err> Next: kestrel login claude --organization Jack
exit> 78
```

An established expiry changes only the first sentence to `Not ready: Claude Code's sign-in
in Profile jack has expired.` Established lack of coverage changes it to `Not ready:
Profile jack's plan does not cover the selected Claude Code model.` Coverage offers inspection
of the selected model before sign-in; it must not promise that signing in again buys coverage.

### No GitHub Integration and no Project

```text
$ kestrel
out> Not ready: no repository is available.
out> Client: http://localhost:7719
err> Next: kestrel github connect
exit> 78
```

The primary repair connects GitHub. `start --repository <public-url>` is also supported;
its review says Sessions can read that repository but cannot push.

### GitHub connected, no Project

```text
$ kestrel
out> Not ready: choose a repository for the first Project.
out> Client: http://localhost:7719
err> Next: kestrel start
exit> 78
```

### Unused broken sign-in, usable alternative

```text
$ kestrel
out> Ready · Organization Jack · Codex · Profile jack
out> Client: http://localhost:7719
err> Warning: Claude Code's sign-in in Profile jack needs attention.
err> Next: kestrel login claude
exit> 0
```

### Selected image lacks the harness

```text
$ kestrel start --harness claude --brief "Fix the README"
err> Cannot start: image ghcr.io/openkestrel/kestrel-env:custom does not carry Claude Code.
err> The image declares: opencode.
err> Next: inspect the configured image and its dev.kestrel.harnesses label.
exit> 78
```

The selected path is blocked even when another harness is usable. The image is not changed
automatically; this contextual gap complements the ordered initial setup gaps.

## start at a terminal: one complete journey

Empty install, no clone. Numbered choices accept the number or harness name. Repository
names/default branches below are from the Integration, not guessed. Vendor URL is a fixture.

```text
$ kestrel start
err> What is your name?
err> Operator:
in> Jack
err> Saved Operator Jack and Organization Jack.
err> Choose a harness: 1 Claude Code · 2 Codex · 3 OpenCode
err> Harness:
in> claude
err> Sign in: 1 Claude subscription · 2 Anthropic API key · 3 Paste subscription token
err> Sign-in Method:
in> 1
err> A Claude plan serves only your own work.
err> Open this URL in your browser:
err>   https://claude.ai/oauth/authorize?fixture=relay
err> Paste the code here; it will not be shown:
in> [hidden code]
err> Checking Claude Code with one model call...
err> Saved Claude Code sign-in in Profile jack, owned by Operator Jack.
err> Connect GitHub: 1 GitHub App · 2 Personal access token · 3 Public repository URL
err> Repository access:
in> 1
err> Open this URL to create and install the GitHub App:
err>   http://localhost:7719/setup/github
err> Waiting for the GitHub Integration...
err> GitHub connected.
err> Choose a repository: 1 example/widgets (main) · 2 example/docs (trunk)
err> Repository:
in> 1
err> What should the Session do?
err> Brief:
in> Fix the README
err> Inferred: Organization Jack (sole) · Project widgets (repository) · Agent claude (harness) · branch main (repository default) · model harness default · Profile jack (Operator).
err> Already saved: Operator Jack · Claude Code sign-in · GitHub Integration.
err> Apply:
err>   Declare Project widgets: example/widgets, base branch main.
err>   Declare Agent claude: Claude Code, harness default model.
err>   Open a Workspace and enqueue its first Session with Brief: Fix the README
err> Start? [y/N]
in> y
out> Workspace widgets-1 · Session claude-1 queued
out> Client: http://localhost:7719
exit> 0
```

The relay subflow here is identical to `login`; GitHub is identical to `github connect`.
Setup changes take effect when each step is saved. The final prompt confirms the start
plan only. A later refusal does not roll back saved setup. Initial setup completes only
when the confirmed start succeeds; queued is not a claim the harness has begun working.

### Existing clone and saved setup: folded plan

```text
$ kestrel start --brief "Fix the README"
err> Inferred: Organization Jack (sole) · repository example/widgets (origin) · branch main (origin default) · Project widgets (existing) · Agent claude (existing) · model harness default · Profile jack (Operator).
err> Apply:
err>   Open a Workspace and enqueue its first Session with Brief: Fix the README
err> Start? [y/N]
in> y
out> Workspace widgets-2 · Session claude-2 queued
out> Client: http://localhost:7719
exit> 0
```

### Explicit Profile and replacement

```text
$ kestrel start --profile jack-work --credential ANTHROPIC_API_KEY --brief @brief.txt
err> Inferred: Organization Jack (sole) · repository example/widgets (origin) · branch main (origin default) · Project widgets (existing) · Agent claude (existing) · model harness default.
err> Sign-in: Profile jack-work (explicit).
err> Apply:
err>   Replace Organization Jack's Provider Credential ANTHROPIC_API_KEY from the environment.
err>   Open a Workspace and enqueue its first Session with Brief from brief.txt.
err> Start? [y/N]
in> y
out> Workspace widgets-3 · Session claude-3 queued
out> Client: http://localhost:7719
exit> 0
```

The replacement is not presented as the selected sign-in: the explicit Profile is selected.
The proposed review also prints a given repository, branch, model or harness when supplied,
so a one-line inferred summary cannot hide overrides.

### Decline final confirmation

```text
$ kestrel start --brief "Fix the README"
err> Inferred: Organization Jack (sole) · repository example/widgets (origin) · branch main (origin default) · Project widgets (existing) · Agent claude (existing) · model harness default · Profile jack (Operator).
err> Apply:
err>   Open a Workspace and enqueue its first Session with Brief: Fix the README
err> Start? [y/N]
in> n
err> No Workspace or Session was created.
exit> 0
```

When this follows first-run setup, add `Saved setup remains available for your next start.`
Never say “nothing changed” after naming the Operator or saving a sign-in.

## start without a terminal

### Missing information: aggregate everything known, never wait

Empty install, outside a clone. No write has occurred. The rerun is a template, not executable
as printed; placeholders are not invented Operator names or repositories.

```text
$ kestrel start </dev/null
err> Cannot start without a terminal: setup information is missing.
err>   --operator <name>
err>   --harness <harness> --sign-in <method> --credential <environment-variable>
err>   --repository <url>
err>   --brief <text-or-@file>
err>   --yes to apply the reviewed plan without prompting
err> Next: supply these values and rerun:
err>   kestrel start --operator <name> --harness <harness> --sign-in <method> --credential <environment-variable> --repository <url> --brief @brief.txt --yes
exit> 78
```

An existing setup with only a missing Brief is invalid CLI input, exit 2, not a setup gap.
Request only missing values. A known repository default supplies the branch; if unavailable,
name `--branch <branch>` instead of guessing. Subscription credentials supplied this way
land in the Operator's Profile, even though the input flag is named `--credential`.

### Fully specified API-key path

`ANTHROPIC_API_KEY` is already set securely in the caller's environment.

```text
$ kestrel start --operator Jack --harness claude --sign-in anthropic-api-key --credential ANTHROPIC_API_KEY --repository https://github.com/example/widgets.git --brief @brief.txt --yes </dev/null
err> Checking Claude Code with one model call...
err> Saved Operator Jack and Organization Jack.
err> Saved Anthropic API key as Organization Jack's Provider Credential.
err> Inferred: Project widgets (repository) · branch main (repository default) · Agent claude (harness) · model harness default.
err> Given: Operator Jack · harness claude · repository https://github.com/example/widgets.git · Sign-in Method anthropic-api-key · Brief from brief.txt.
err> Sign-in: Organization Jack's Provider Credential ANTHROPIC_API_KEY.
err> Warning: this public repository can be read; Sessions cannot push without a GitHub Integration.
err> Apply:
err>   Declare Project widgets: example/widgets, base branch main.
err>   Declare Agent claude: Claude Code, harness default model.
err>   Open a Workspace and enqueue its first Session with Brief from brief.txt.
out> Workspace widgets-1 · Session claude-1 queued
out> Client: http://localhost:7719
exit> 0
```

Method ids in this prototype are proposed catalogue spellings, to be validated in the sign-in
specification slice. `--yes` suppresses confirmation; it does not supply missing information.
With complete input but no `--yes`, proposed behavior is exit 2 with
`Cannot confirm a start without a terminal. Next: rerun with --yes after reviewing the plan.`
No setup writes occur before this preflight check.

## status

```text
$ kestrel status
out> Client: http://localhost:7719
out> Ready · Organization Jack · Claude Code · Profile jack
out> Workspaces: 3 open · 1 sealed
out> Sessions: 1 queued · 0 unbriefed · 1 working · 0 trailing · 1 waiting · 4 ended · 0 unreachable
exit> 0
```

```text
$ kestrel status
out> Client: http://localhost:7719
out> Not ready: no harness is signed in.
out> Workspaces: 0 open · 0 sealed
out> Sessions: 0 queued · 0 unbriefed · 0 working · 0 trailing · 0 waiting · 0 ended · 0 unreachable
err> Next: kestrel start
exit> 78
```

Without an Organization yet, omit counts rather than imply a scoped successful count.
Failed is an Outcome, not a Session phase; it is included in ended, not a new phase.

## login: local and SSH use

Always print the relay URL on its own line. On a local machine, try to open the browser
and print the same URL even if that fails. Under SSH, do not open a browser or require
a tunnel for vendor sign-in. The token/file is captured inside the relay, never printed.

### Claude Code locally

```text
$ kestrel login claude
err> A Claude plan serves only your own work.
err> Open this URL in your browser:
err>   https://claude.ai/oauth/authorize?fixture=relay
err> Paste the code here; it will not be shown:
in> [hidden code]
err> Checking Claude Code with one model call...
out> Signed in to Claude Code · Profile jack · Operator Jack
exit> 0
```

### Claude Code over SSH

```text
$ kestrel login claude
err> A Claude plan serves only your own work.
err> Open this URL in your laptop's browser:
err>   https://claude.ai/oauth/authorize?fixture=relay
err> Paste the code here; it will not be shown:
in> [hidden code]
err> Checking Claude Code with one model call...
out> Signed in to Claude Code · Profile jack · Operator Jack
exit> 0
```

### Codex locally

```text
$ kestrel login codex
err> Open this URL in your browser:
err>   https://auth.openai.com/codex/device
err> Enter this code on that page: ABCD-EFGH
err> Use this code only for the sign-in you just started.
err> Waiting for sign-in; the relay expires in 10 minutes. Ctrl-C cancels.
err> Checking Codex with one model call...
out> Signed in to Codex · Profile jack · Operator Jack
exit> 0
```

### Codex over SSH

```text
$ kestrel login codex
err> Open this URL in your laptop's browser:
err>   https://auth.openai.com/codex/device
err> Enter this code on that page: ABCD-EFGH
err> Use this code only for the sign-in you just started.
err> Waiting for sign-in; the relay expires in 10 minutes. Ctrl-C cancels.
err> Checking Codex with one model call...
out> Signed in to Codex · Profile jack · Operator Jack
exit> 0
```

There is no code pasted back to kestrel for Codex. An earlier vendor expiry wins over the
ten-minute relay ceiling; show the actual expiry when supplied.

### Relay failure and manual fallback

```text
$ kestrel login claude
err> A Claude plan serves only your own work.
err> Open this URL in your browser:
err>   https://claude.ai/oauth/authorize?fixture=relay
err> Sign-in relay failed before a sign-in was saved.
err> Next: kestrel login claude --token
exit> 4
```

The fixture is a typed relay rejection, exit 4. A provisioning outage is exit 5.
Expiry prints `Sign-in relay expired; no sign-in was saved.` and the same retry/manual step,
exit 4. Ctrl-C requests cancellation and prints `Sign-in cancelled; no sign-in was saved.`,
exit 130. Cancellation after a completed save instead says the sign-in was saved.

```text
$ kestrel login claude --token
err> A Claude plan serves only your own work.
err> Paste the subscription token; it will not be shown:
in> [hidden token]
err> Checking Claude Code with one model call...
out> Signed in to Claude Code · Profile jack · Operator Jack
exit> 0
```

```text
$ kestrel login codex --file auth.json
err> Checking Codex with one model call...
out> Signed in to Codex · Profile jack · Operator Jack
exit> 0
```

Reviewed naming: `--token` selects a hidden token/key prompt at a terminal or stdin
otherwise; `--file PATH` reads an existing login file. Neither option starts a relay.
Both import the selected method's material and run the real sign-in check.
`kestrel login claude --method anthropic-api-key` prompts for a hidden key (or reads stdin),
checks it, then prints `Signed in to Claude Code · Provider Credential ANTHROPIC_API_KEY · Organization Jack`.

### Claude relay without a terminal

```text
$ kestrel login claude </dev/null
err> Claude Code's relayed sign-in needs a terminal to receive its code.
err> Next: run kestrel login claude at a terminal, or supply a subscription token with kestrel login claude --token < token.txt
exit> 2
```

Codex can run without a TTY: print the URL/code and wait for browser authorization with
the relay deadline, never read stdin. A missing Operator blocks either login, exit 78,
with `Next: kestrel start --operator <name>`.

### Check cannot establish authentication failure

```text
$ kestrel login codex --file auth.json
err> Checking Codex with one model call...
err> Sign-in saved in Profile jack, but its check could not complete: the model service is unavailable.
err> State: unchecked. This does not establish that your sign-in has expired.
err> Next: kestrel status
exit> 5
```

Review proposal: retain the saved value as unchecked and disclose the partial result. A
typed failed check reports authentication failed, expired or not covered only with evidence,
and exits 4. A subsequent start gets 78 only if this is its blocking setup gap.

## github connect

### Local GitHub App

```text
$ kestrel github connect
err> Open this URL to create and install the GitHub App:
err>   http://localhost:7719/setup/github
err> Waiting for the GitHub Integration... Ctrl-C cancels waiting.
out> GitHub connected · Integration github · Organization Jack
exit> 0
```

The CLI waits for the existing manifest/installation flow's completion. The browser must
be able to reach the Client origin; the vendor-sign-in exception does not apply here.
The route spelling `/setup/github` is proposed, not an existing endpoint commitment.

### SSH GitHub App: tunnel is needed

Fixture `SSH_CONNECTION` identifies the client and server addresses, but not the user's
original SSH host alias. Use the known SSH server address and current remote username;
preserve a non-default server port when reported. Never invent an alias.

```text
$ kestrel github connect
err> GitHub setup needs a browser that can reach kestrel.
err> In a second terminal on your laptop, run:
err>   ssh -N -L 7719:127.0.0.1:7719 jack@203.0.113.10
err> Then open this URL on your laptop:
err>   http://localhost:7719/setup/github
err> Waiting for the GitHub Integration... Ctrl-C cancels waiting.
out> GitHub connected · Integration github · Organization Jack
exit> 0
```

When the SSH destination is unavailable, request it at a terminal or print
`ssh -N -L 7719:127.0.0.1:7719 <ssh-destination>` explicitly as a template. A custom
Client origin/port must produce the matching tunnel and browser URL. If local 7719 is
already occupied, explain a selectable local port rather than silently opening another install.

### Token alternative, hidden prompt and pipe

```text
$ kestrel github connect --token
err> GitHub token; it will not be shown:
in> [hidden token]
err> Checking GitHub access...
out> GitHub connected · Integration github · Organization Jack
exit> 0
```

```text
$ kestrel github connect --token < github-token.txt
err> Checking GitHub access...
out> GitHub connected · Integration github · Organization Jack
exit> 0
```

Proposed `--token` is a boolean selecting hidden input/stdin, not a secret-valued argument.
Registering a token does not claim GitHub App identity; the token-backed Integration is
the alternative the first-run spec already allows. Auth failure prints an evidence-based
rejection and retry command, exit 4. Ctrl-C cancels waiting, not an App already created.

## profile set: missing profile, existing profile and piped input

### Create missing profile at a terminal

```text
$ kestrel profile set jack --variable CLAUDE_CODE_OAUTH_TOKEN
err> Profile jack will be created, owned by Operator Jack.
err> Value for CLAUDE_CODE_OAUTH_TOKEN; it will not be shown:
in> [hidden token]
out> Created Profile jack · Operator Jack
out> Saved CLAUDE_CODE_OAUTH_TOKEN in Profile jack
exit> 0
```

### Replace an existing entry

```text
$ kestrel profile set jack --variable CLAUDE_CODE_OAUTH_TOKEN
err> Replace CLAUDE_CODE_OAUTH_TOKEN in Profile jack; it will not be shown:
in> [hidden token]
out> Saved CLAUDE_CODE_OAUTH_TOKEN in Profile jack
exit> 0
```

### Pipe or redirect the value

```text
$ kestrel profile set jack --variable CLAUDE_CODE_OAUTH_TOKEN < token.txt
out> Created Profile jack · Operator Jack
out> Saved CLAUDE_CODE_OAUTH_TOKEN in Profile jack
exit> 0
```

Strip exactly one final newline sequence from a variable, preserving other whitespace.
For `--file .codex/auth.json < auth.json`, preserve all file bytes. An empty variable or
file is refused with `No value was supplied. Next: provide a value on stdin or rerun at a terminal.`,
stderr, exit 2; do not create a Profile first. A missing Operator is exit 78 unless
`--owner <name>` explicitly supplies another owner. An existing Profile is not re-owned.

Low-level Profile writes do not pretend to have performed the catalogue's real sign-in
check. Proposed output says saved; `login --token` or `login --file PATH` is the checked first-run path.

## Unreachable control plane and scope

### Known Compose deployment

```text
$ kestrel status
err> Cannot reach the control plane at http://127.0.0.1:7718.
err> Next: docker compose ps
err> Then: docker compose logs kestrel
exit> 5
```

If deployment knowledge establishes Compose is stopped, the primary action becomes
`docker compose up -d`. No guessed claim that it is stopped based on connection refusal alone.

### Custom target, deployment unknown

```text
$ kestrel status --control-plane http://127.0.0.1:8818 --organization Jack
err> Cannot reach the control plane at http://127.0.0.1:8818.
err> Check that the control plane is listening there and that any SSH tunnel is open.
err> Next: kestrel status --control-plane http://127.0.0.1:8818 --organization Jack
exit> 5
```

`--verbose` adds sanitized transport evidence. No raw error chain or Compose command for
an unknown deployment. A lost response to a write says the action may have completed and
offers inspection; it does not automatically repeat the write.

```text
$ kestrel status
err> Choose an Organization: Jack, Work.
err> Next: kestrel status --organization <name>
exit> 3
```

Every corrective command retains explicit connection and Organization scope. For example,
a selected Claude sign-in gap under the custom target gives
`kestrel login claude --control-plane http://127.0.0.1:8818 --organization Jack`.

## A failed Session: successful reads, actionable evidence

```text
$ kestrel session show claude-2
out> Session claude-2 · Workspace widgets-2 · ended · failed
out> Claude Code could not authenticate using Profile jack, owned by Operator Jack.
out> Evidence: the harness reported authentication required.
out> Only this sign-in was marked as needing attention.
out> Next: kestrel login claude
exit> 0
```

```text
$ kestrel session show claude-2
out> Session claude-2 · Workspace widgets-2 · ended · failed
out> Image ghcr.io/openkestrel/kestrel-env:custom has no claude-agent-acp.
out> It declares the harnesses: claude, codex, opencode.
out> Next: inspect the configured image and its dev.kestrel.harnesses label.
exit> 0
```

If only arbitrary harness prose exists, display it as diagnostic evidence and offer inspection
of the Session; do not classify a sign-in as expired, not covered or failed from that prose.
These `Next:` lines are part of a successful Session read on stdout, not CLI refusals.

## Decisions for live review

The settled [refusal decision](https://github.com/openkestrel/kestrel/issues/490#issuecomment-5985132323)
already owns exit mappings, evidence, scope and typed next steps. The
[release decision](https://github.com/openkestrel/kestrel/issues/496) still owns packaging,
installation and version selection; these transcripts make no release-policy choice.

Proposed amendments to [the first-run spec](https://github.com/openkestrel/kestrel/issues/460):

1. `start` may collect the Brief at a terminal. Without a terminal it aggregates all known
   missing inputs and requires explicit `--yes` before any setup write. Present behavior
   silently applies a fully specified non-TTY start without `--yes`; this is a deliberate change.
2. Review separates already-saved setup from pending start declarations. Declining the
   final confirmation retains setup and creates no Workspace. Given values and selected
   sign-in remain visible alongside the folded inference line.
3. Add `login --token` and `login --file PATH`; terminal token/key input is hidden,
   pipes supply tokens/keys, and a file path supplies existing login-file contents. Claude relay requires a terminal; Codex can
   wait for device authorization without one. Tokens do not travel in argument values.
4. Treat `github connect --token` as an input mode, and derive a usable SSH tunnel from
   known connection details. Explicitly label unknown SSH destinations as placeholders.
5. A failed sign-in check may leave a saved unchecked value; say so and return the failure
   code. Low-level `profile set` remains a storage write, not an implied successful sign-in check.
6. Use the stream/colour conventions above. Keep initial readiness to one blocking gap,
   while non-TTY start lists all missing information it can establish.

These amendments need the human's reaction before changing the spec or closing the ticket.
Route spelling, Sign-in Method ids and precise tunnel-port selection remain proposed contract
details for this review; they must be locked or carried into the corresponding specification slice.

## Live review so far

- Non-interactive `start` requires explicit `--yes` before any writes, including setup.
- Declining final confirmation retains saved setup and creates no Workspace or Session.
- The checked import fallback is named `login --token` / `login --file PATH`, replacing
  the draft's vague `--manual`. Secret values never appear in command arguments.

Other proposals above remain under review.
