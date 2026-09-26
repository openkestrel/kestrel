# Acceptance attempt: kestrel works the backlog

The live run for `0.2`'s acceptance ticket
([#111](https://github.com/jtmthf/kestrel/issues/111)) proved parallel dispatch, its limit,
non-retroactive Trigger declaration, and idle sealing. It did not close the rung: two successful
Sessions lost all of their work when their disposable Environments were reaped. That finding became
[#124](https://github.com/jtmthf/kestrel/issues/124) and then the durable-Instance specification
[#125](https://github.com/jtmthf/kestrel/issues/125). The `ROADMAP.md` marker remains at `0.2` until
that work lands and this acceptance run is repeated.

The test used the real Compose stack, GitHub integration, and `ready-for-agent` issues on this
repository. It began at commit `941ee0a`, the last planned implementation ticket in the rung, and
preserved the named volume from earlier dogfood runs so Trigger declaration was tested against live
history rather than an empty database.

## 1. Build and upgrade the live stack

The current control plane and supervisor were built with the GitHub-capable Environment image:

```sh
docker compose build
docker build --file images/kestrel-env-github/Dockerfile --tag kestrel-env-github .
docker compose up -d
```

A local, gitignored `compose.override.yaml` selected `kestrel-env-github`. Recreating the
control-plane container left the `kestrel` volume intact. The existing `acme` organization,
`kestrel` project, `builder` agent, `origin` GitHub integration, two sealed Workspaces, and recorded
Events all survived. The default concurrency limit of two was unchanged.

The base image does not put Rust on its default `PATH`. OpenCode provisioned Rust 1.96.0 under
`/home/kestrel/.cargo/bin`, and the agents ran Cargo through its generated environment script.
An initial base-shell inspection missed that distinction; the resulting report
[#122](https://github.com/jtmthf/kestrel/issues/122) was corrected and closed.

## 2. Declare the Trigger against recorded history

Before declaration the store contained a `ready-for-agent` Event for
[#109](https://github.com/jtmthf/kestrel/issues/109), recorded on September 9, and only the two
sealed Workspaces. The Trigger was declared on September 10:

```sh
kestrel trigger declare ready --organization acme --repository jtmthf/kestrel \
  --label ready-for-agent --project kestrel --agent builder
```

Six observations over a full GitHub poll interval continued to show the same two Workspaces. Trigger
`ready` (`01a08d5d-24fc-7ad3-9030-e65f629bdda4`) opened nothing for #109 or any other Event recorded
before its declaration.

## 3. Put three issues on the backlog together

Issues [#79](https://github.com/jtmthf/kestrel/issues/79),
[#80](https://github.com/jtmthf/kestrel/issues/80), and
[#104](https://github.com/jtmthf/kestrel/issues/104) were unblocked and agent-ready. Their labels
were removed, then `ready-for-agent` was added to all three in parallel. Nobody opened a Workspace,
enqueued a Session, or directed an agent after that labelling.

One poll recorded all three Events and opened all three Workspaces at `2026-09-10T22:10:12Z`.

| Issue | Workspace | Session | First observed state |
| --- | --- | --- | --- |
| #80 | `01a08d5f-10fa-7410-aeb3-c0e22a67379b` | `01a08d5f-10fb-7823-ba23-ec1632dc4ea9` | active |
| #104 | `01a08d5f-10fd-7200-b753-f78df4cb6d99` | `01a08d5f-10fd-7200-b753-f79f0c8f61ef` | active |
| #79 | `01a08d5f-10fe-76f1-bddc-ba282e7a271b` | `01a08d5f-10ff-7962-9395-1d349e6ecb28` | queued |

Docker showed exactly the two active Sessions' containers. A third never appeared while both slots were
held. When #104 ended at `22:13:48Z`, #79 became active on the next sweep while #80 remained active.
The live run therefore exercised both sides of the limit: excess work waited, then dispatched as
soon as capacity became available.

## 4. Outcomes

All three initial Sessions reported `succeeded`. #104 opened
[PR #121](https://github.com/jtmthf/kestrel/pull/121) and its outcome returned to the issue without
operator action. Its agent also wrote an ordinary issue comment before the Session ended; the integration
treated that as an operator follow-up and ran the Workspace a second time. That feedback hazard is the
non-blocking finding [#123](https://github.com/jtmthf/kestrel/issues/123).

The #79 and #80 Sessions made and tested substantial repository changes, but neither committed or pushed
them. #80's final report explicitly said no commit or pull request was made because none was
requested. Kestrel recorded success and destroyed both Environments, leaving no recoverable copy of
either change. A process exit was successful, but the backlog issues were not durably worked.

This is the blocking finding [#124](https://github.com/jtmthf/kestrel/issues/124). Its accepted design
is [#125](https://github.com/jtmthf/kestrel/issues/125): a Workspace-owned Instance outlives its Sessions,
the supervisor reports git state, and Kestrel never reaps an Instance holding work that exists
nowhere else. A prompt asking an agent to commit would improve one agent's odds, but it would not
provide the observable durability guarantee #125 requires.

## 5. Idle sealing

The named volume and stack were left running. All three Workspaces sealed themselves after their real
24-hour idle windows, without an explicit `workspace seal`:

| Issue | Last active | Sealed |
| --- | --- | --- |
| #80 | `2026-09-10T23:04:53Z` | `2026-09-11T23:09:55Z` |
| #104 | `2026-09-10T22:48:10Z` | `2026-09-11T22:53:35Z` |
| #79 | `2026-09-10T22:48:28Z` | `2026-09-11T22:53:35Z` |

A later `ready-for-agent` Event opened a Workspace for #125. Its Session held a slot from
`2026-09-10T22:51:01Z` until its lease expired at `2026-09-11T01:12:00Z`, then that Workspace also
sealed itself at `2026-09-12T01:12:00Z`. The failed Session is further live evidence for #125's rule
that missing supervisor state must be treated as unrecoverable rather than safe to destroy.

## Result

The scheduler behavior requested by #111 worked live: one labelling batch opened three Workspaces,
two Sessions overlapped under a limit of two, the queued Session took the released slot, declaration did not
replay retained history, and the Workspaces later sealed themselves. The rung remains open because
successful Sessions can still destroy their only copy of completed work. After #125 lands, repeat the
durability portion of this run and move the `ROADMAP.md` marker to `0.3` only if several backlog
issues leave recoverable branches or pull requests.

## Follow-up attempt: three harnesses and unpublished work

On 25–26 September 2026, the acceptance gate in [#186](https://github.com/jtmthf/kestrel/issues/186)
was exercised against a pre-existing local Compose stack. This was a second attempt, not a replacement
for the September 10 run above. The stack held an `acme` Organization, a `kestrel` Workspace against
this repository, and OpenCode, Codex, and Claude Agents. Private Subscription Profiles were declared
for the operator's own logins. Their values are absent from this record.

The stack's running control-plane container was four days old, exposed no host operator port, and its
`organization declare` command had no live-Instance limit option although this checkout's Client
does. Its exact source revision was not established. The Sessions below checked out `9a5358d`, but
the running control plane cannot be claimed as a build of that revision. This limits what this
attempt can establish about the current code.

| Check | Live observation |
| --- | --- |
| Codex subscription persistence | The opt-in `codex_answers_on_a_chatgpt_login_before_and_after_a_restart` smoke passed: two real model answers on a personal ChatGPT login, separated by control-plane and Instance replacement. |
| Codex skill-led, non-PR work | Session `01a0dbc5-bf63-7791-95b2-a9e529769c59` opened Run `01a0dbc5-eb2c-7d02-8ccb-36feb996fd82` on `gpt-6-astra`. Its first instruction named the repository's `diagnosing-bugs` skill and asked for a diagnosis of profile-dependent model selection. The first Turn answered without a code change; an operator follow-up about Cargo's absolute path resumed the **same Run ID**. Codex reproduced the defect through the real library and finished the diagnosis; the Run ended succeeded. |
| OpenCode build and test | Session `01a0dbc3-bc2a-7110-be4a-dea0306af70b` used the default `opencode/big-pickle` model. It reported `cargo check --locked --workspace --all-targets` exit 0, one focused `continuity.rs` test passed, and `cargo clippy --locked --workspace --all-targets -- -D warnings` exit 0, all in a clean checkout. This Run does not count as subscription-backed access. |
| OpenCode Go subscription work | Session `01a0dbcb-427b-7f22-a8e9-71adcf42b70c` fixed `opencode-go/glm-5.3` and the private Go profile. Run `01a0dbcb-6622-7991-bf42-2a9522e3fb9a` completed a non-PR acceptance review and reported `cargo fmt --all -- --check` exit 0 in the checkout. It ended succeeded. |
| Claude | Session `01a0dbc4-0051-7d63-95e5-9df1cfa89ada` failed first with `Authentication required` using the local login file. A retry with the same login's OAuth access token failed with HTTP 401: the token had expired. A direct local Claude call also reported that the OAuth session could not refresh. The operator is renewing the personal login. No successful Claude task is yet recorded. |
| Active-run limit | While the Codex and OpenCode Go Runs were active, the OpenCode durability Run `01a0dbcb-d5f2-72f0-8423-d5e6bf6220e0` stayed queued and began only after a slot was released. |
| Unpublished work through failure | The durability Run created only `ACCEPTANCE-CANARY.txt`, untracked and unpushed, with the 29-byte contents `acceptance canary 2026-09-25\n`. Kestrel's Instance list named that one untracked file as the reason to retain `docker/kestrel-01a0dbc3-ddff-7690-bd6b-4f1d5dc2ef40`. Stopping the Run mid-turn recorded a failure. Follow-up Run `01a0dbcd-54df-7212-9c3f-0b973d160ca1` reused the same Instance and verified the file's exact contents and untracked state. |

The first OpenCode Session could not switch to Go after it opened: the model is fixed for a Session.
Before its profile-backed Run, the Organization's cached OpenCode model list contained only free
models, and `agent model builder --model opencode-go/glm-5.3` was refused. The profile-backed Run
refreshed the list, after which selection succeeded for the new Go Session. Codex reproduced the
profile-insensitive validation with a temporary database and tests. This non-blocking finding is
filed as [#294](https://github.com/jtmthf/kestrel/issues/294).

### Current-checkout stack

To remove the version ambiguity above, three images were built from this checkout and started as
an isolated Compose project, `kestrel-acceptance-186`, with its own data volume and link network.
Its operator boundary was published at `127.0.0.1:7728`; the pre-existing stack and its retained
canary were left intact. This project declared a one-Instance limit and an OpenCode Go Agent using
the operator's private Go profile.

Session `01a0dbd4-01d8-71f3-b36f-a6bc011936da` created one untracked, unpushed file,
`ACCEPTANCE-CANARY-186.txt`, containing `current build canary\n` (21 bytes; its first report
miscounted the length). Kestrel listed the untracked file as the reason for retaining Instance
`docker/kestrel-01a0dbd4-2e31-7b63-9344-8a1cd291dd11`.

While that Instance was held, Codex Session `01a0dbd5-0374-78e1-84a0-ed255d47ed6a` queued Run
`01a0dbd5-2e29-7410-b44c-23f40f03ff95`. Its reported wait reason was: "the organization acme
has reached its limit of 1 live Instance; none idle is known recoverable". The OpenCode Session
continued on its existing Instance under the same cap and checked the canary unchanged. An operator
then stopped Run `01a0dbd7-3c91-7db0-8221-212bcffa96a5` mid-turn; it failed explicitly, and
Kestrel kept the Instance because no Run had reported a recoverable checkout after that failure.
Follow-up Run `01a0dbd7-e62f-7b72-8e38-7b273d76c771` reused that same Instance and verified
the file's exact contents and untracked state. The canary was deliberately removed only after this
verification, so a clean Instance could be reclaimed for queued work.

The cleanup Turn did report a clean checkout and an answer, but its supervisor then exited with
code 137 before its final report. Kestrel marked that Run failed, archived the now recoverable
Instance, and admitted the queued Codex Run. No unpublished work was lost; the unexplained
false-negative Run outcome is filed as [#295](https://github.com/jtmthf/kestrel/issues/295).
The queued Codex Run `01a0dbd5-2e29-7410-b44c-23f40f03ff95` then completed a non-PR review of
the capacity logic on the personal ChatGPT login and ended succeeded.

For a literal skill-led Brief, a dispatch-only `skill-led-review` Trigger with the Codex profile
was explicitly dispatched on [#294](https://github.com/jtmthf/kestrel/issues/294). Session
`01a0dbdc-5731-7e13-b2e6-22738e8563aa` begins with a `brief` transcript entry naming
`.agents/skills/diagnosing-bugs/SKILL.md`; its Run is `01a0dbdc-5732-7021-ab57-d7cbc14d6d8b`.
The Brief asks for a non-PR diagnosis on a real Kestrel issue.

The GitHub Integration recorded #294's `needs-triage` label Event before the `history-check`
Trigger was declared against that label. No Session opened for it during the observation window;
the Trigger was then disabled. The earlier September 10 run remains the stronger historical-label
check because it also watched a full poll interval with multiple old ready-for-agent Events.

The earlier run remains the live evidence for simultaneous issue labelling and 24-hour idle
sealing. This attempt adds two-harness subscription-backed work, same-Run follow-up, a skill-led
non-PR task, build and test, and current-build failure and cap survival of unpublished work. It has
**not** established a completed Claude task or several newly worked backlog issues leaving
recoverable branches or pull requests. The `ROADMAP.md` marker therefore remains at `0.2`.
