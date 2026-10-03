# How developer tools guide a first run and name the fix

Research for [#484](https://github.com/openkestrel/kestrel/issues/484), checked 3 October 2026 against the source of `gh` (`cli/cli`), `flyctl` (`superfly/flyctl`), the Supabase CLI (`supabase/cli`, `apps/cli-go`), the Vercel CLI (`vercel/vercel`, `packages/cli`), Tailscale (`tailscale/tailscale`), Coder (`coder/coder`), the Docker client (`moby/moby`) and the Codex CLI (`openai/codex`), all on their default branches; Anthropic's Claude Code documentation (the CLI is not open source); [clig.dev](https://clig.dev/); RFC 8628; `sysexits(3)`; and the GOV.UK, PatternFly and Primer design systems. Quoted output is copied from source string literals; `%s` marks a substituted value. Docker Desktop's and Tailscale's in-app browser onboarding were not covered: neither publishes the flow as source or as first-party design guidance.

The patterns below are grouped by the [0.4 spec](https://github.com/openkestrel/kestrel/issues/460) surface each one feeds. Each ends with a recommendation for the setup-flow and CLI prototypes: **adopt**, **adapt** or **reject**.

## 1. Readiness: an ordered list of checks, each with its fix

**`fly doctor`** runs checks in dependency order, prints `Testing authentication token... ` and then `PASSED` or `FAILED`, and on a blocking failure prints the fix in prose and **stops**, because later checks would fail for the same reason ([`doctor.go`](https://github.com/superfly/flyctl/blob/master/internal/command/doctor/doctor.go)):

```
Testing authentication token... FAILED
(Error: ...)

We can't authenticate you with your current authentication token.

Run 'flyctl auth login' to get a working token, or 'flyctl auth signup' if you've
never signed up before.
```

A check that is not a blocker says so instead of failing: `Testing local Docker instance... Nope`, and with `--verbose`, `This is fine, we'll use a remote builder.` The same checks render as JSON under `--json`, a map of check name to `"ok"` or the error ("This JSON output is (unfortunately) depended on in production").

**Tailscale** reduces its backend state to one sentence that names the next step ([`status.go`](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/status.go)): `Tailscale is stopped.`, `Logged out.` followed by `Log in at: %s` when a login URL is pending, `Machine is not yet approved by tailnet admin.`

**Claude Code** splits a read-only diagnosis from a fixer: `claude doctor` "print[s] read-only installation and settings diagnostics from the terminal without starting a session", while `/doctor` inside a session "proposes fixes it can apply after you confirm" ([CLI reference](https://code.claude.com/docs/en/cli-reference), [Troubleshooting](https://code.claude.com/docs/en/troubleshooting)). `claude auth status` prints JSON by default, `--text` for people, and "exits with code 0 if logged in, 1 if not"; `claude daemon status` "exits 1 if the supervisor isn't running". **Codex**'s `codex login status` prints `Not logged in` and exits 1 ([`login.rs`](https://github.com/openai/codex/blob/main/codex-rs/cli/src/login.rs)).

**Supabase** leads `supabase status` with what works: `supabase local development setup is running.`, then the URLs, with any `Stopped services: [...]` listed above them ([`status.go`](https://github.com/supabase/cli/blob/develop/apps/cli-go/internal/status/status.go)).

**For kestrel.**
- **Adopt** fly's ordering and its stop at the first blocking gap. The spec's `GET /operator/readiness` already orders the gaps, and bare `kestrel` prints the first gap and its fix.
- **Adopt** Tailscale's one sentence per state. Each gap's `sentence` is what bare `kestrel` prints, so the browser and the CLI word it identically.
- **Adopt** an exit status on bare `kestrel` and `kestrel status`, as `claude auth status` has: 0 when ready and non-zero when there's a gap, so a script can gate on it.
- **Adopt** a JSON form (`--json`) that is the readiness document itself. Agents will drive kestrel's CLI as often as people do.
- **Reject** a separate `kestrel doctor` for 0.4. Bare `kestrel` is the doctor, and a second command would drift from it. Fly's "This is fine" line is worth keeping for checks that aren't blockers, such as a sign-in still `unchecked`.

## 2. A server that isn't there: diagnose it, then name the start command

The Docker client turns a failed connection into one line that names the address and the likely cause ([`moby/client/errors.go`](https://github.com/moby/moby/blob/master/client/errors.go)):

```
Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?
```

Tailscale goes further. On a failed connection it **looks for the daemon's process** before it words the error, and adds a hint for the platform ([`diag.go`](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/diag.go)):

```
failed to connect to local tailscaled; it doesn't appear to be running (sudo systemctl start tailscaled ?)
failed to connect to local Tailscale service; is Tailscale running?            # macOS
failed to connect to local tailscaled (which appears to be running as %v, pid %v). Got error: %w
```

The last form appears only when the process *is* running, so the person learns the problem is something other than "start it".

`gh` hides the raw error unless asked. A DNS failure prints `error connecting to api.github.com` and `check your internet connection or https://githubstatus.com`; the underlying error is printed only with debug on ([`internal/ghcmd/cmd.go`](https://github.com/cli/cli/blob/trunk/internal/ghcmd/cmd.go)). Supabase answers a missing stack with `supabase start is not running.`; when Docker itself is unreachable, it adds `Docker Desktop is a prerequisite for local development. Follow the official docs to install: https://docs.docker.com/desktop` ([`misc.go`](https://github.com/supabase/cli/blob/develop/apps/cli-go/internal/utils/misc.go), [`docker.go`](https://github.com/supabase/cli/blob/develop/apps/cli-go/internal/utils/docker.go)).

**For kestrel.**
- **Adopt** the spec's line (`kestrel isn't running at <url>. Start it with docker compose up -d, or set KESTREL_CONTROL_PLANE.`), with the chain behind `--verbose` as `gh` does.
- **Adapt** Tailscale's probe. Before choosing the sentence, the CLI can tell the cases apart cheaply: refused at a loopback URL (start it), refused at a remote URL (check `KESTREL_CONTROL_PLANE`), DNS failure, TLS or HTTP-version mismatch, and something answering that isn't kestrel. Each gets its own sentence. The browser's "kestrel isn't running" page has only the `502` to go on, so it names `docker compose ps` and `docker compose logs kestrel` as the spec says.

## 3. Error format: cause, then the fix, last

[clig.dev](https://clig.dev/#errors): "Catch errors and rewrite them for humans," as in "Can't write to file.txt. You might need to make it writable by running 'chmod +w file.txt'." It continues: "Put the most important information at the end of the output," and "Suggest commands the user should run."

The tools that do this well keep the fix **separate from the message**, so every error path can attach one:

- **Supabase** keeps a `CmdSuggestion` that any code path may set. The root command prints the error in red and then the suggestion on its own line. When nothing set one and `--debug` is off, it falls back to `Try rerunning the command with --debug to troubleshoot the error.` ([`cmd/root.go`](https://github.com/supabase/cli/blob/develop/apps/cli-go/cmd/root.go), [`misc.go`](https://github.com/supabase/cli/blob/develop/apps/cli-go/internal/utils/misc.go)). A port conflict names the container holding the port: `Try stopping the running project with supabase stop --project-id %s`, or `docker stop %s`.
- **`gh`** prints the error, and after a `401` adds `Try authenticating with:  gh auth login`. A command run before login prints `To get started with GitHub CLI, please run:  gh auth login` and `Alternatively, populate the GH_TOKEN environment variable with a GitHub API authentication token.` The wording changes with context: under `GITHUB_ACTIONS=true` it shows the `env: GH_TOKEN: ${{ github.token }}` snippet instead ([`root/help.go`](https://github.com/cli/cli/blob/trunk/pkg/cmd/root/help.go)).
- **Vercel**'s formatter takes `{ message, link, action }` and renders `Error: <message>` with `Learn More: <url>` on the next line ([`output/error.ts`](https://github.com/vercel/vercel/blob/main/packages/cli/src/util/output/error.ts)).
- **Tailscale** prints the exact corrected command when a person would otherwise have to work it out. `Error: changing settings via 'tailscale up' requires mentioning all non-default flags. ... use the command below ...` is followed by a full `tailscale up --flag=value ...` line ([`up.go`](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/up.go)). Claude Code's `claude rm`, when it refuses, "prints the exact flag and value to pass" ([CLI reference](https://code.claude.com/docs/en/cli-reference)).
- **Claude Code's docs are indexed by exact error string.** The troubleshooting pages open with a "What you see | Solution" table, for example `` `OAuth error` or `403 Forbidden` `` → Fix authentication ([Troubleshoot installation and login](https://code.claude.com/docs/en/troubleshoot-install)).

**For kestrel.**
- **Adopt** a fix field on the error type, beside the message, mirroring the readiness gap's `fix`. The operator API's error body carries `fix: { command, action, docs? }`, and the CLI prints the message and then the fix as the last line. One renderer covers readiness and refusals.
- **Adopt** printing the exact command, with the person's own values filled in (Tailscale, `claude rm`, Vercel's `buildCommandWithYes`). An example is a Session failing on authentication: `kestrel login claude`, not "sign in again".
- **Adopt** a usage document indexed by exact error string. The USAGE.md rewrite can carry a "What you see" table for the five or six errors a first run can hit.
- **Reject** a `Learn More` URL on every error. Kestrel has no hosted docs site, and a link into the repository goes stale with the branch. A link is worth adding only where the fix happens on another site, such as the OpenCode console.

## 4. Every prompt has a flag; without a terminal, fail naming it

clig.dev: "Only use prompts or interactive elements if `stdin` is an interactive terminal (a TTY)," "Never *require* a prompt. Always provide a way of passing input with flags or arguments," and "If `--no-input` is passed, don't prompt or do anything interactive." Four ways to carry that out:

- **`gh`** checks per command and fails with a usage error, then prints the command's usage ([`issue/create`](https://github.com/cli/cli/blob/trunk/pkg/cmd/issue/create/create.go), [`cmd.go`](https://github.com/cli/cli/blob/trunk/internal/ghcmd/cmd.go)):
  ```
  must provide `--title` and `--body` when not running interactively
  --yes required when not running interactively
  ```
  When an AI agent is detected as the caller, `gh` prints the **full** help instead of the terse usage, "giving AI agents the examples, JSON fields and environment variables they need to correct themselves without a second round trip".
- **flyctl** has its prompt helpers return a sentinel, `prompt.ErrNonInteractive`, when there is no TTY. Each call site turns it into a sentence naming the flag ([`prompt.go`](https://github.com/superfly/flyctl/blob/master/internal/prompt/prompt.go)): `name argument must be specified when not running interactively`, `username must be specified with --username flag when not running interactively`, `--yes flag must be specified when not running interactively`. The prompt and its flag cannot drift apart without the compiler or a test noticing.
- **Coder** drives prompts and flags from **one table** of `{flag, env, value, prompt}`. Without a TTY it reports **every** missing field at once instead of failing on the first ([`cli/login.go`](https://github.com/coder/coder/blob/main/cli/login.go)):
  ```
  --first-user-trial requires trial info that cannot be prompted for in a non-interactive environment; set --first-user-trial-first-name (or CODER_FIRST_USER_TRIAL_FIRST_NAME), --first-user-trial-last-name (or ...), ...
  ```
  The comment explains why it decides up front: some prompt widgets swallow EOF and "would block forever waiting for input that never arrives".
- **Vercel** has an explicit `--non-interactive` mode for agents and CI. A missing value or a needed confirmation writes one JSON object to stdout, `{ status: "action_required", reason, message, next: [{ command, when }] }`, and exits. Its implementer guide requires `next[].command` to be "the exact command the user ran, with `--yes` added", with `<angle-bracket>` placeholders only where a value is truly unknown and global flags preserved ([`non-interactive-mode.md`](https://github.com/vercel/vercel/blob/main/packages/cli/docs/non-interactive-mode.md), [`ensure-link.ts`](https://github.com/vercel/vercel/blob/main/packages/cli/src/util/link/ensure-link.ts)). The human form is `Command vercel link requires confirmation. Use option --yes to confirm.`

**For kestrel.**
- **Adopt** Coder's single table for `kestrel start`'s gaps: each row is a gap kind, its flags, its prompt and its environment variable. Without a terminal, list **every** missing flag in one error, so a script author fixes the invocation once.
- **Adopt** Vercel's re-run line. End the error with the person's own command line plus the missing flags as `<placeholders>`, for example `kestrel start --operator <name> --harness <harness> --sign-in <method> --credential <ENV_VAR>`.
- **Adapt** `gh`'s usage-on-misuse. Print the subcommand's usage after a non-interactive refusal, and with `--json`, emit the `next[]` object instead.
- **Reject** a `--no-input` flag for now. TTY detection plus the flags covers the spec's stories, and a flag is cheap to add later.

## 5. Secrets: hidden prompt at a terminal, standard input otherwise, checked on save

clig.dev: "Do not read secrets directly from flags ... the flag value will leak the secret into `ps` output and potentially shell history," and "If you're prompting for a password, don't print it as the user types."

- **Codex** refuses a secret flag whose standard input is a terminal, instead of waiting for Ctrl-D, and shows the pipe to use; it trims the value and refuses an empty one ([`login.rs`](https://github.com/openai/codex/blob/main/codex-rs/cli/src/login.rs)):
  ```
  --with-api-key expects the API key on stdin. Try piping it, e.g. `printenv OPENAI_API_KEY | codex login --with-api-key`.
  Reading API key from stdin...
  No API key provided via stdin.
  ```
- **`gh auth login --with-token < mytoken.txt`** reads the token from standard input; `--with-token` together with `--web` is refused as a usage error ([`auth/login`](https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/login/login.go)).
- **Coder**'s `Paste your token here:` prompt is `Secret: true`. It validates the token with one real API call before accepting it, and re-asks on `That's not a valid token!` ([`cli/login.go`](https://github.com/coder/coder/blob/main/cli/login.go)). It also refuses to log in while an environment variable would override the result: `CODER_SESSION_TOKEN is set. This environment variable takes precedence over any session token stored on disk.` followed by `unset CODER_SESSION_TOKEN`. Claude Code documents the same trap: an `ANTHROPIC_API_KEY` in the shell silently overrides the subscription ([Troubleshoot installation and login](https://code.claude.com/docs/en/troubleshoot-install)).

**For kestrel.**
- **Adopt** all of it for `kestrel profile set` and `kestrel login`: a hidden prompt at a terminal, and standard input read to the end otherwise, with the trailing newline stripped and an empty value refused.
- **Adopt** Codex's message when a pipe was expected but standard input is a terminal. This replaces today's wait for Ctrl-D.
- **Adopt** Coder's validate-before-accept at a terminal, using the spec's one real call on save. At a prompt, re-ask on `expired` or `not_covered` rather than saving a sign-in that is known to be broken.
- **Adapt** the override warning. When a Session's environment carries both the subscription variable and a key, as `ANTHROPIC_API_KEY` would beside `CLAUDE_CODE_OAUTH_TOKEN`, readiness should name which one wins.

## 6. Show the plan before applying it

`fly launch` prints a table of what it will create. Each row is **label, value and where the value came from**, followed by `Do you want to tweak these settings before proceeding?` Under `--yes`, or without a TTY, it goes ahead ([`launch/cmd.go`](https://github.com/superfly/flyctl/blob/master/internal/command/launch/cmd.go), [`state.go`](https://github.com/superfly/flyctl/blob/master/internal/command/launch/state.go)). The sources are short phrases from [`plan_builder.go`](https://github.com/superfly/flyctl/blob/master/internal/command/launch/plan_builder.go); the values and layout below are illustrative:

```
We're about to launch your app on Fly.io. Here's what you're getting:

Organization: Jack Moore          (fly launch defaults to the personal org)
Name:         my-app              (specified on the command line)
Region:       ...                 (...)
App Machines: shared-cpu-1x, 1GB  (from your fly.toml)
Postgres:     <none>              (not requested)
```

When the plan has problems the CLI cannot settle, it prints `The following problems must be fixed in the Launch UI:` and asks `Would you like to continue in the web UI?`. If the person declines, it aborts: `launch can not continue with errors present`.

PatternFly's wizard guidance says the same of browser flows: "Always end with a review step. Summarize what was entered so it can be confirmed before submit," and label the last button with "a more specific verb or verb-object pair when you can, such as **Create**" ([Wizard design guidelines](https://www.patternfly.org/components/wizard/design-guidelines)).

**For kestrel.**
- **Adapt** fly's table. The spec folds inferences onto one line, then lists only what will be declared, held, opened or replaced. Keep fly's short source phrases (`from this clone`, `the sole Organization`, `--profile`) in that inferred line, and print the full per-row table only under `--verbose`. Fly's table has eight rows; kestrel's had eighteen, which is the reason the spec folds it.
- **Adopt** the hand-off to the browser for anything the CLI can't settle, as in fly's `continue in the web UI?`. For kestrel this is GitHub when there is no display: print the `kestrel open` URL or the `ssh -L` command rather than giving up.
- **Adopt** a review step before the browser setup's final action, labelled with the action, such as **Start Session**.

## 7. Signing in from a terminal over SSH

Two shapes exist, and kestrel's relay needs both: a **device code**, where the person types a code shown in the terminal into a page, and a **paste-back**, where the page shows a code that the person types into the terminal.

**Device code.** `gh auth login` ([`authflow/flow.go`](https://github.com/cli/cli/blob/trunk/internal/authflow/flow.go)):

```
! First copy your one-time code: ABCD-1234
Press Enter to open https://github.com/login/device in your browser...
```

Without a TTY it doesn't wait for Enter. It prints `Open this URL to continue in your web browser: %s`. If the browser fails to open, it prints `! Failed opening a web browser at %s` and `Please try entering the URL in your browser manually`. With `--clipboard` it prints `! One-time code (ABCD-1234) copied to clipboard`.

`codex login --device-auth` numbers the steps, gives the expiry, and warns against phishing ([`device_code_auth.rs`](https://github.com/openai/codex/blob/main/codex-rs/login/src/device_code_auth.rs)):

```
Follow these steps to sign in with ChatGPT using device code authorization:

1. Open this link in your browser and sign in to your account
   <verification_url>

2. Enter this one-time code (expires in 15 minutes)
   ABCD-EFGH

Continue only if you started this login in Codex. If a website or another person gave you this code, cancel.
```

When device login isn't enabled for the account it falls back with `Device code login is not enabled; falling back to browser login.`

RFC 8628 grounds these choices. Even when a QR code carries the full URL, "Clients MUST still display the `user_code`, as the authorization server will require the user to confirm it" (§3.3.1). User codes should use a vowel-free alphabet, `BCDFGHJKLMNPQRSTVWXZ`, dashed for reading, such as `WDJB-MJHT` (§6.1). The server should "inform the user that they are authorizing a device" against remote phishing (§5.4) ([RFC 8628](https://www.rfc-editor.org/rfc/rfc8628.html)).

**Paste-back.** Claude Code, over SSH or in a container: "After you sign in, the browser shows a login code instead of redirecting back automatically. Paste that code into the terminal at the `Paste code here if prompted` prompt." Pressing `c` copies the OAuth URL, which "also works when the URL wraps across lines in a narrow or SSH terminal and can't be clicked directly." If pasting into the interactive prompt does nothing, `claude auth login` "reads the pasted code from standard input". A truncated paste fails with `OAuth error: Invalid code. Please make sure the full code was copied` ([Troubleshoot installation and login](https://code.claude.com/docs/en/troubleshoot-install)).

Coder tries to open the browser and says which of the two things happened ([`cli/login.go`](https://github.com/coder/coder/blob/main/cli/login.go)):

```
Open the following in your browser:

	https://coder.example.com/cli-auth

Paste your token here:
```

When the browser did open, the first line reads `Your browser has been opened to visit:`. The page it opens says "Copy the session token below and **paste it in your terminal.**" and has a copy button that changes to "Session token copied!" ([`CliAuthPageView.tsx`](https://github.com/coder/coder/blob/main/site/src/pages/CliAuthPage/CliAuthPageView.tsx)).

Tailscale puts the URL on a line of its own between blank lines: `\nTo authenticate, visit:\n\n\t%s\n\n`. `--qr` adds a terminal QR code. `--json` emits a first object with `AuthURL` and a data-URL `QR` while `BackendState` is `NeedsLogin`, then a second object once the login lands ([`up.go`](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/up.go)). The second object lets a wrapping program, kestrel's browser included, follow the login without screen-scraping.

**For kestrel.**
- **Adopt** printing the URL **alone on its own indented line, between blank lines**, as Coder, Tailscale and Codex do. A wrapped URL is the most common SSH failure that Claude Code documents.
- **Adopt** printing the URL every time, even after opening a browser (Coder's "has been opened to visit"). Skip opening when `SSH_CONNECTION` is set or there's no display, as the spec says.
- **Adopt** Codex's numbered steps, its expiry and its "Continue only if you started this login" line for `codex-device-auth` (`awaiting_browser { url, code }`). Show the code in the browser's relay card in the same words.
- **Adopt** Claude Code's prompt wording for `claude-setup-token` (`awaiting_code`): `Paste the code shown after you sign in:`. Read the code from standard input when standard input isn't a terminal, which is Claude Code's own fallback. Map an invalid or truncated code to the full-copy hint.
- **Adapt** Tailscale's two-object `--json` for `kestrel login --json`, matching the relay's `GET …/relay/{id}/events` states one to one.
- **Reject** QR codes and clipboard writes for 0.4. Over SSH, the clipboard is the remote machine's, and Claude Code documents how unreliable OSC 52 is ([Troubleshooting](https://code.claude.com/docs/en/troubleshooting)). A QR code is cheap to add later.

## 8. Exit codes

- **`gh`** documents its codes in `gh help exit-codes`: 0 success, 1 failure, 2 cancelled, 4 "requires authentication", and 8 for a pending state such as checks still running ([`help_topic.go`](https://github.com/cli/cli/blob/trunk/pkg/cmd/root/help_topic.go), [`cmd.go`](https://github.com/cli/cli/blob/trunk/internal/ghcmd/cmd.go)). "No results" is not a failure and exits 0.
- **Claude Code** and **Codex**: status commands exit 0 when signed in and 1 when not.
- **Vercel** exits with the `exitCode` carried on the `action_required` payload, 1 by default.
- **`sysexits(3)`** defines 64 `EX_USAGE`, 69 `EX_UNAVAILABLE` ("A service is unavailable"), 75 `EX_TEMPFAIL`, 77 `EX_NOPERM` and 78 `EX_CONFIG` ("Something was found in an unconfigured or misconfigured state"). FreeBSD's page now calls the interface "deprecated and is retained only for compatibility" and says "its use is discouraged" ([sysexits(3)](https://man.freebsd.org/cgi/man.cgi?query=sysexits)).

**For kestrel.** **Adapt** `gh`: document a short list and keep it short.
- 0 success.
- 1 failure.
- 2 usage, which is `clap`'s default.
- 69 control plane unreachable, as the spec says. It is the one sysexits value a script really branches on, and the number is precedent enough despite the deprecation note.
- One code for "not ready", returned by bare `kestrel` and `kestrel status` when readiness has a gap and by any command refused for one. Either `gh`'s 4 or `EX_CONFIG` 78 works; choose one and document it with `kestrel help exit-codes` or in USAGE.md.

**Reject** a code per gap kind. The JSON readiness document carries the kind.

## 9. Browser first run: a gate, then one question per step, then fix-it states

- **Setup is a gate that removes itself.** Coder routes to `/setup` while the deployment has no first user. Once setup is complete, `/setup` redirects to `/login`, or into the app when the person is already signed in ([`SetupPage.tsx`](https://github.com/coder/coder/blob/main/site/src/pages/SetupPage/SetupPage.tsx)). The CLI's counterpart is `Your Coder deployment hasn't been set up!` followed by `Would you like to create the first user?`. Without a TTY, every first-user field is a flag (`--first-user-email`, `--first-user-username`, ...), and a missing one fails with `the initial user cannot be created in non-interactive mode. use the API` ([`cli/login.go`](https://github.com/coder/coder/blob/main/cli/login.go)). This is the spec's "setup shows while there's no Operator."
- **One question per step, with a way back.** GOV.UK: "Asking just one question per question page helps users understand what you're asking them to do," and "Always include a Back link ... to reassure them it's possible to go back and change previous answers" ([Question pages](https://design-system.service.gov.uk/patterns/question-pages/)).
- **Step indicators are disputed.** GOV.UK warns against step-list indicators: they "are often not noticed, take up lots of space, do not scale well on small screens ... make it hard to handle conditional sections". PatternFly's wizard shows a sidebar of numbered steps in which "later steps are usually disabled so the flow stays sequential" ([Wizard](https://www.patternfly.org/components/wizard/design-guidelines)). Kestrel's setup has four steps, and one of them is conditional: GitHub, or a public URL.
- **After setup, a task list rather than a wizard.** GOV.UK's task list is for when people "cannot or prefer not to complete all tasks in one session". Each row shows a status, and "the whole row is linked", because people tried to click the status ([Task list](https://design-system.service.gov.uk/components/task-list/)).
- **Empty states that are never dead ends.** Primer separates first-time use ("convey intention in a welcoming, human way"), temporary emptiness and error states. Each gets one primary action, and "Only offer an alternative path forward that can help the user complete their task, resolve the error, or get more context on the error" ([Empty states](https://primer.style/product/ui-patterns/empty-states/)). Today's `No Organization exists yet. kestrel start declares one.` is an error state with no action in the browser, which this rule forbids.
- **Hand-off pages say where the code goes.** Coder's `/cli-auth`: "Copy the session token below and paste it in your terminal," with a copy button that confirms ([`CliAuthPageView.tsx`](https://github.com/coder/coder/blob/main/site/src/pages/CliAuthPage/CliAuthPageView.tsx)).

**For kestrel.**
- **Adopt** Coder's gate: the setup route exists only while readiness reports "no Operator", and redirects away once the Operator exists.
- **Adopt** one question per step with Back: name, harness and Sign-in Method, repository, Brief.
- **Adapt** the step indicator. Use a short text label (`Step 2 of 4: Sign in`) rather than a PatternFly-style sidebar. It costs no space and survives the conditional GitHub step, which is GOV.UK's objection.
- **Adopt** Primer's rule for every readiness gap after setup: a sentence and one action that fixes it. In the Client, the action is the API call. Show the CLI command beside it as secondary text, so a CLI user reading the browser learns it.
- **Reject** a persistent task list for 0.4. After setup, at most one or two gaps exist at a time, and the spec's banners and empty states cover them. Revisit when readiness grows past five gap kinds.

## 10. Output for agents

Two of the surveyed CLIs now shape errors for an agent caller. `gh` prints full help on misuse when it detects an agent (§4). Vercel's `--non-interactive` JSON contract exists so that "Agents ... can use suggested commands without manual substitution" (§4). `claude auth status` defaults to JSON (§1).

**For kestrel.** **Adopt** a `--json` on bare `kestrel`, `kestrel status` and every refusal. It emits the readiness document or the error with its `fix`, unchanged from the operator API, so an agent driving kestrel reads the same fields the browser renders. Kestrel's own agents and the people scripting it are both first-class callers.

## Summary for the prototypes

| Surface | Pattern | Source | Verdict |
| --- | --- | --- | --- |
| Bare `kestrel` | Ordered checks, stop at first blocker, one sentence and one fix | fly doctor, Tailscale status | Adopt |
| Bare `kestrel` | Exit 0 ready / non-zero not ready; `--json` | `claude auth status`, `codex login status` | Adopt |
| Unreachable control plane | Probe, then a sentence per cause; chain only under `--verbose` | Tailscale `diag.go`, `gh` | Adapt |
| Every error | Message, then a separate fix line, last | Supabase `CmdSuggestion`, `gh`, clig.dev | Adopt |
| Every error | The exact command with the person's values | Tailscale `up`, `claude rm`, Vercel | Adopt |
| Every error | `Learn More` URL | Vercel | Reject |
| `kestrel start` | One table of gap → flag/env/prompt; list all missing at once | Coder `login.go` | Adopt |
| `kestrel start` | Re-run line with `<placeholders>` | Vercel `non-interactive-mode.md` | Adopt |
| `kestrel start` | Plan with sources per row | `fly launch` | Adapt: sources in the one inferred line |
| Secrets | Hidden prompt, or stdin; refuse a TTY where a pipe was expected | Codex, `gh`, clig.dev | Adopt |
| Secrets | Validate before accepting; re-ask | Coder | Adopt |
| `kestrel login` | URL alone on its own line; always printed | Coder, Tailscale, Codex | Adopt |
| `kestrel login codex` | Numbered steps, expiry, phishing line | Codex, RFC 8628 | Adopt |
| `kestrel login claude` | `Paste code` prompt; stdin fallback | Claude Code | Adopt |
| `kestrel login` | QR, clipboard | `gh`, Tailscale | Reject for 0.4 |
| Exit codes | 0, 1, 2, 69, one "not ready" code, documented | `gh help exit-codes`, sysexits | Adapt |
| Browser setup | Gate that removes itself | Coder `/setup` | Adopt |
| Browser setup | One question per step, Back, review step | GOV.UK, PatternFly | Adopt |
| Browser setup | Sidebar stepper | PatternFly | Reject in favour of `Step n of 4` text |
| After setup | Fix-it empty states, one primary action, no dead ends | Primer | Adopt |
| After setup | Task list | GOV.UK | Reject for 0.4 |
