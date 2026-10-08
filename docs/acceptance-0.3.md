# Acceptance attempt: kestrel's work is joinable mid-flight

The `0.3` acceptance gate ([#426](https://github.com/openkestrel/kestrel/issues/426)) was run on
3–4 October 2026 at `5da5466`, on this repository's `ready-for-agent` work. **The gate is not
met.** Three scripted checks fail and two pass only in part; one critical and one serious axe
violation appear on live Workspaces; and a lapsed lease could be explained only from the control
plane's log. The `ROADMAP.md` marker stays at `0.3`; the blockers are under [Result](#result).
A [rerun](#rerun-the-blockers-fixed) from 6 October at `4d0bfa2` exercised what this attempt left
failing or untried, and **meets the gate**.

## The stack

The images were built from this checkout and run as an isolated Compose project beside any other
stack, with its own volume, link network and image tags:

```sh
export COMPOSE_PROJECT_NAME=kestrel-accept03 KESTREL_VOLUME=kestrel-accept03-data \
  KESTREL_LINK_NETWORK=kestrel-accept03-link KESTREL_OPERATOR_PORT=7738 KESTREL_CLIENT_PORT=7739 \
  KESTREL_CONTROL_IMAGE=kestrel-accept03 KESTREL_CLIENT_IMAGE=kestrel-client-accept03 \
  KESTREL_ENV_IMAGE=kestrel-env-accept03
docker compose -f compose.yaml -f accept03.override.yaml build
docker build --file images/kestrel-dev/Dockerfile --build-arg KESTREL_ENV=kestrel-env-accept03 \
  --tag kestrel-dev-accept03 .
docker compose -f compose.yaml -f accept03.override.yaml up -d
```

`accept03.override.yaml`, kept outside the repository, set `KESTREL_IMAGE: kestrel-dev-accept03`,
published `/webhooks` at `127.0.0.1:7737`, and, after the queue checks, set
`KESTREL_MAX_ACTIVE_SESSIONS: "3"` (the default is 2).

| Declared | Value |
| --- | --- |
| Organization | `acme`, a live-Instance limit of 3, raised to 5 once the cap wait was recorded |
| Project | `kestrel`, `openkestrel/kestrel` on `main` |
| Agents | `builder` (OpenCode; its harness default was `opencode/fledge-alpha-free`), `claude`, `codex` |
| Subscription Profile | `jack`: a Codex `auth.json` and a `claude setup-token` token |
| Credential | `GH_TOKEN`, the operator's `gh` token |
| Integration | `origin`, polled, inbound and outbound |
| Trigger | `.kestrel/triggers.yaml` as committed, adding `profile: jack` |

The browser was driven by Playwright scripts in `packages/client/acceptance/`, run with `bun`
from `packages/client` against the live stack. `KESTREL_CLIENT_URL` (default
`https://127.0.0.1:7739`), `KESTREL_CONTROL_PLANE` and `KESTREL_ORGANIZATION` choose the stack;
`act.ts` writes its screenshot to `KESTREL_SHOTS`.

| Script | What it does |
| --- | --- |
| `follow.ts` | Follows one Workspace as a named person; logs every streamed event with its arrival time, and the Session header every 15 s. |
| `act.ts` | Takes one composer action as a named person: post, Send now, interrupt, edit, withdraw, or a first Brief. |
| `open.ts` | Opens an unbriefed Workspace through the New Workspace form and watches it get ready. |
| `queue.ts` | Reads the queue in the browser, then with `kestrel queue` and the operator read. |
| `work.ts` | Reads a Workspace's Work pane: summary, each diff scope, files, Sessions. |
| `axe.ts` | Audits the list, the form, and each Workspace in every Transcript mode and Work tab, at 1280 and 375 px. |
| `keyboard.ts` | The keyboard-only path on live data, counting key presses. |

Arrival lag was computed from `follow.ts` logs: the first time the browser received a unit,
minus the unit's recorded completion (`completion.finished_at`, or `finished_at` for an Activity
summary). The host and control-plane clocks agreed to within 0.2 s.

## Scripted checks

### Live Transcript — passes, except an expired payload

| Measure | Result |
| --- | --- |
| Completed units reaching a following browser | 536 units on Claude (#258), Codex (#134) and OpenCode (#90): p50 0.07 s, p99 0.16 s, max 1.62 s. None over 5 s. |
| A long tool call while running | A 90-step `for … sleep 1` loop stayed in the live line as `in_progress` for its whole run; Codex's `gh pr checks --watch` showed as running for 283 s and as a background-task unit. |
| Completed, failed and unresolved status | Activities carried completed and failed counts (`12 tools, 3 failed`) and the interrupted Turn's status. |
| Detail and a large payload on demand | Expanding an Activity fetched only that range (`first_seq=16&last_seq=20`). "Load 91 KiB payload" fetched all 92,884 bytes. |
| An expired payload | **Not exercised.** Tombstones need 30-day-old entries; only the scripted e2e (`activities.spec.ts`) covers it. |

### Late join and reconnect — passes

Followers that joined Workspaces holding 295 to 401 entries received shared state and Activity
summaries only; no narration or detail was fetched until an Activity was opened. The control
plane was replaced twice while three followers were attached (20:40:34Z and 01:21:37Z). Each
follower reconnected within 2 s, every Session survived, and the received sequences had no gap
and no duplicate entry (checked against the Activity ranges). Presence was re-established on
reconnect.

### Two people and turns — passes

Browsers declared as `jack` and `alex`, and a CLI follower (`--as-participant sam`), followed
Claude's working Session on #258 together; the header read "watching alex, jack, sam". Over the
window all three were attached, each received the same 24 shared-state entries in the same order.
Alex's message, posted mid-Turn, appeared to jack as a Held Message without Edit or Withdraw
controls; alex's edit showed to jack as "edited", and alex's withdrawal removed it for both.
Sam's turn from the CLI was held behind the Session's own work and waited for a slot. Interrupt,
from the browser, moved OpenCode's #90 Session from Working to Waiting without ending it, and a
later message continued it. When alex's follow closed, presence dropped alex within one second.

The two-browser and browser-with-CLI cases ran together rather than separately, and no second
human took part.

### Open and continue — **fails**

The browser cannot open a Workspace without a Brief. The New Workspace form always sends the
person's name as `participant`, and the control plane refuses a participant on an open that
carries no Brief: `422 a participant names the author of a brief, and this open carries none`
(`crates/kestrel/src/workspace.rs`). Every browser e2e test of the form supplies a Brief.

The rest was exercised on a Workspace opened unbriefed from the CLI. Its Session reached
`harness_ready` within 45 s while every Active-Work Slot was held by others, and the queue listed
it under unbriefed with no slot. The first message, sent from the browser's Brief composer, became
the Brief with jack as its author. A message to a Waiting Session became that Session's next
Turn: Codex on #134 asked a scoping question and was answered from the browser. How the browser
sets a continued Session's Turns apart from the Workspace's history was not examined.

Trailing was seen twice. After the fixture Session answered, it held its slot as Trailing for
35 s with no unit listed, then became Waiting. Claude on #258 answered "I'll check back once
[CI] completes" and became Waiting 30 s later, with no unit naming the pending check; ten minutes
on, Claude woke on its own, ran two tool calls and answered again
([#513](https://github.com/openkestrel/kestrel/issues/513)). Neither shows a Trailing Session
exposing its open background work.

### Work state — passes, except the pull request

The fixture Workspace held one local commit, one staged file, one changed file, one untracked
file and one stash. The browser's Work pane showed exactly that (*nothing pushed*, *1 commit*,
*1 file* staged and changed, *1 untracked*, *1 stash*), each diff scope, the commit with its
message, the stash, a directory with tracked and untracked marks and sizes, and the untracked
file's contents. Stopping the Instance's container made the Work pane say "the Instance isn't
answering" and the Diff view answer 504; the Session ended when its lease lapsed, and the
Instance list held the checkout for "1 unpushed commit, 2 uncommitted changes, 1 untracked file,
1 stash".

**A learned pull request was not shown.** kestrel learns pull requests only from signed webhook
deliveries; a polled Integration answers "pull requests unavailable" on every Workspace, although
agents opened #497, #498 and #499. A second, webhook-only Integration was prepared but not
registered during the run.

### Options and continuity — passes in part

| Check | Result |
| --- | --- |
| Requested and effective model differ | Codex: "requested harness default · effective gpt-6-astra". Claude reports its alias as effective (`sonnet`, or `default`), although `/context` named `claude-sonnet-5`. |
| Title, commands, options, live usage | Shown, and kept current, on all three harnesses. |
| An option change between Turns | Changing Model to Haiku warned "the next turn re-read the context without the prompt cache: 32,494 tokens"; the change was recorded as `option_changed`. |
| A change refused mid-Turn | `409 … is working, and an option cannot change mid-turn`; the browser disables the controls. |
| A Held Message beginning with a command | `/context`, held behind a working Turn, ran as its own Turn before alex's next message. |
| Losing ACP continuity | **Not exercised.** Killing the harness inside the Instance was not permitted to the agent running the gate. |

### Queue — **fails**

Three Delegated Work Items against two slots, then a fourth against a live-Instance limit of 3,
were read back to back: the browser first (about 2 s per Workspace), then `kestrel queue`.

| Case | `kestrel queue` | Browser |
| --- | --- | --- |
| Ready | `1 calm-valley-abfecule queued ready` | "Queued #1"; "Queued at position 1." |
| Slot limit | `limit 2; 2 occupied (noble-falcon-xeszjeod, merry-summit-jrmqlktv)` | "Slots 2/2" |
| Instance limit | `limit 3; 3 counted (docker/kestrel-…)` | "Instances 3/3" |
| Instance-cap wait | `queued … at the limit of 3 live Instances` | "Waiting: at the live Instance limit of 3." |
| A Waiting Session with input | listed after the queue: `merry-summit-jrmqlktv waiting … input since …; all 2 Active-Work Slots occupied` | "Next Turn at position 1. Waiting: every Active-Work Slot is occupied (2)." |
| Serialized profile and cap together | `the Subscription Profile jack is held by mighty-beacon-wnqydomg; at the limit of 5 live Instances` | not captured |

A Waiting Session's input carries a slot position, shared with queued requests, but sits in its
own section. No estimated start time appeared anywhere. Only one Session was ready at a time, so
FIFO numbering across several was not shown, and no read checked that it archived nothing. Two
parts fail:

- **No dependency wait can be produced.** Nothing outside the test support declares a Session
  dependency (`declare_blocked` is called only from `crates/kestrel/tests/support`); no operator
  endpoint, Trigger or command writes one. #259 and #260 started ahead of their blocker #258
  because operator dispatch records `worked_ahead`: Trigger readiness, not a Session dependency.
- **The browser does not name what occupies each limit.** It shows counts; the CLI names the
  occupying Sessions and Instances.

### Accessibility and narrow screens — **fails**

CI's axe suite passed on [#469](https://github.com/openkestrel/kestrel/pull/469), the change
`5da5466` merged, but it audits empty Workspaces. Run against live ones (`axe.ts`, 52 audits), it
found:

- **critical `button-name`**: the Transcript's scroll-to-bottom button
  (`components/ai-elements/conversation.tsx`) has no accessible name. It renders only once a
  Transcript is long enough to scroll.
- **serious `scrollable-region-focusable`**: every tool input and result `<pre>` in Steps and
  Full (416–445 per Workspace) scrolls but cannot be focused.
- moderate `landmark-one-main`, `page-has-heading-one` and `region`.

No view scrolled horizontally at 375 px. The keyboard-only path *find a running Session → follow
it → open a diff → take a turn* completed on live data at 1280 px (174 key presses, 52 Tabs from
the Diff view to the composer) and at 375 px (114). Whether live updates move focus or announce
every tool update was not checked. VoiceOver was not run on a live Transcript; the operator's
VoiceOver validation in [`acceptance-0.3-accessibility.md`](acceptance-0.3-accessibility.md) was
on fixture Workspaces.

## The dogfood stretch

The stretch was compressed, with the operator's agreement, from five calendar days to one run
across two evenings, and followed by scripted browsers rather than a person at the screen. Ten
Delegated Sessions came from the `delegated` Trigger, by dispatch or by an `@kestrel` comment;
eight were followed while they worked, and two were still queued when this was written.

| Enqueued (UTC) | Issue | Harness and model | Workspace | Session | Outcome |
| --- | --- | --- | --- | --- | --- |
| 3 Oct 20:35 | [#258](https://github.com/openkestrel/kestrel/issues/258) | Claude, default | `coral-wren-zeeeepoe` | `noble-falcon-xeszjeod` | failed: the Claude subscription's session limit |
| 3 Oct 20:36 | [#134](https://github.com/openkestrel/kestrel/issues/134) | Codex, `gpt-6-astra` | `lucky-fern-lwtsvvtm` | `merry-summit-jrmqlktv` | asked a scoping question, was answered from the browser, opened [#497](https://github.com/openkestrel/kestrel/pull/497); then failed on a lapsed lease |
| 3 Oct 20:36 | [#90](https://github.com/openkestrel/kestrel/issues/90) | OpenCode, `fledge-alpha-free` | `keen-juniper-cvkrlnhn` | `calm-valley-abfecule` | interrupted and continued; then failed: "the agent was cancelled" |
| 3 Oct 20:44 | [#261](https://github.com/openkestrel/kestrel/issues/261) | OpenCode, `fledge-alpha-free` | `fable-maple-ftiqpzta` | `nimble-willow-swudjtql` | waited on the Instance cap, opened [#498](https://github.com/openkestrel/kestrel/pull/498); then failed on a lapsed lease |
| 4 Oct 01:12 | #258 | Claude, default | `coral-wren-zeeeepoe` | `lively-kite-jgmotrho` | opened [#499](https://github.com/openkestrel/kestrel/pull/499); waiting |
| 4 Oct 01:12 | #90 | OpenCode, `fledge-alpha-free` | `keen-juniper-cvkrlnhn` | `merry-fox-mohmpkmk` | working, after its plan was approved from the browser |
| 4 Oct 01:34 | [#259](https://github.com/openkestrel/kestrel/issues/259) | Codex, `gpt-6-astra` | `noble-badger-bqsklvkm` | `mighty-beacon-wnqydomg` | working, ahead of its blocker #258 |
| 4 Oct 01:35 | [#260](https://github.com/openkestrel/kestrel/issues/260) | OpenCode, `fledge-alpha-free` | `daring-robin-rzmzymog` | `jolly-juniper-cktzcirb` | working, ahead of its blocker #258 |
| 4 Oct 01:38 | #134 | Codex | `lucky-fern-lwtsvvtm` | `happy-yarrow-vsabtuzt` | from an `@kestrel` comment; queued behind the profile and the Instance limit |
| 4 Oct 01:38 | #261 | OpenCode | `fable-maple-ftiqpzta` | `merry-robin-fbceapme` | from an `@kestrel` comment; queued behind the Instance limit |

Three operator Sessions on the work-state fixture (`rapid-harbor-ghikosmx`) are not counted.

Observations:

- **Two healthy Sessions were failed by a host sleep.** Codex's #134 and OpenCode's #261 Sessions
  had answered and opened their pull requests, then waited. The host slept from about 00:10 to
  00:19Z; on waking, the lease sweep ended both at the same instant, before either supervisor
  could renew, and kestrel posted "session failed" under each successful answer on the issues.
  The browser named the lapsed lease; *why* it lapsed was learned only from the control plane's
  log. This is a stretch moment that needed a reach past kestrel.
- **An OpenCode Turn ended "cancelled" with no one cancelling it.** Its last narration was about
  to ask for confirmation of its plan, as the TDD skill directs; the question never reached
  kestrel, and the Session failed.
- **Codex asked before guessing.** On #134 it stopped to ask which of two conflicting scopes was
  meant; the answer from the browser composer became its next Turn.
- **The Claude subscription limit** ended one Delegated and one operator Session mid-work; the
  browser showed the reset time, and Sessions after the reset continued on the same Instances.
- **Turn comments carry every narration line**, not only the answer.

## Findings

Gate blockers:

- [#500](https://github.com/openkestrel/kestrel/issues/500): the browser cannot open a Workspace
  without a Brief.
- [#502](https://github.com/openkestrel/kestrel/issues/502): no Session dependency can be
  declared, so the queue's dependency wait cannot happen.
- [#503](https://github.com/openkestrel/kestrel/issues/503): the browser's queue does not name the
  occupants of each limit.
- [#504](https://github.com/openkestrel/kestrel/issues/504): a critical (`button-name`) and a
  serious (`scrollable-region-focusable`) axe violation on live Workspaces.
- [#505](https://github.com/openkestrel/kestrel/issues/505): a host sleep fails healthy Sessions,
  and explaining it needed the control plane's log.

Other findings:

- [#501](https://github.com/openkestrel/kestrel/issues/501): the New Workspace form always shows
  "The browser Client failed": `opening.error !== undefined` is true for TanStack Query's `null`.
- [#506](https://github.com/openkestrel/kestrel/issues/506): a polled Integration can never learn a
  pull request, and `github-automation.md` does not say so.
- [#507](https://github.com/openkestrel/kestrel/issues/507): an Integration cannot be changed or
  removed: `register` refuses an existing name, and there is no delete.
- [#508](https://github.com/openkestrel/kestrel/issues/508): OpenCode's question cancels the Turn
  and fails the Session.
- [#509](https://github.com/openkestrel/kestrel/issues/509): Claude's effective model is its alias.
- [#510](https://github.com/openkestrel/kestrel/issues/510): a command Turn's output is recorded
  twice, with no Turn outcome.
- [#513](https://github.com/openkestrel/kestrel/issues/513): a Claude Session reads Waiting while
  its agent has a background check scheduled.
- [#511](https://github.com/openkestrel/kestrel/issues/511), workbench polish:
  - The Session header lists every offered command with its hint; Claude's take half the header.
  - Narration renders Markdown as raw text: fences, tables and emphasis.
  - An Activity's accessible name runs its parts together ("Activity 16–205 tools3.2s").
  - A Session Trailing through its quiet period shows no reason.
  - With the Instance stopped, the Work pane drops its last known summary, and the Sessions
    tab shows a fresh lease until it lapses.
- [#512](https://github.com/openkestrel/kestrel/issues/512): moderate axe findings:
  `landmark-one-main`, `page-has-heading-one`, `region`.

## Result

The gate is **not met**, and the `ROADMAP.md` marker stays at `0.3`. Live Transcript, late join
and reconnect, and two people and turns hold on live work across all three harnesses, and work
state holds apart from the pull request. Before a rerun, the blockers above must be fixed, and
these must be exercised:

- a learned pull request on a webhook Integration;
- a deliberate loss of ACP continuity;
- a Trailing Session exposing open background work;
- a dependency wait, with several Sessions ready in FIFO order and a read that archives nothing;
- an expired payload read on a live Transcript;
- live updates that neither steal focus nor announce every tool update, and a VoiceOver pass on
  a live Transcript;
- ten Delegated Sessions each followed while they work.

## Rerun: the blockers fixed

The gate was run again on 6 October 2026 at `4d0bfa2`, after #500 and #502–#506 closed. It
re-checked only what the first attempt left failing or unexercised; live Transcript arrival, late
join, two people and turns, work state and the options checks held at `5da5466` and were not
repeated. **At `4d0bfa2` the gate stops again**: axe found 15 serious violations on live
Workspaces. This branch fixes them; with the fix every scripted check passes, and the VoiceOver
pass, run on 8 October, passes too (see
[Accessibility](#accessibility-and-narrow-screens--passes-after-a-fix) and [Result](#result-1)).
The `ROADMAP.md` marker moves to `0.4`.

### The stack

A fresh isolated project, `kestrel-accept03b`, built from `4d0bfa2` (migrations changed in place
since `5da5466`, so the first attempt's volume could not be reused), on ports 7748 and 7749 with
`KESTREL_IMAGE: kestrel-dev-accept03b`. The client image was rebuilt with the accessibility fix
for the last axe and keyboard runs.

| Declared | Value |
| --- | --- |
| Organization | `acme`: a live-Instance limit of 3, raised to 5 and then 9 once the cap waits were recorded |
| Active-Work Slots | 2, raised to 4 at 05:06Z |
| Agents | `builder` (OpenCode, `opencode/muse-spark-1.3-contributor-free`), `claude`, `codex` |
| Subscription Profile | `jack`: a Codex `auth.json` and a `claude setup-token` token |
| Integration | `origin`, a GitHub App created through `kestrel integration create-github-app` and polled |
| Trigger | `.kestrel/triggers.yaml` as committed, adding `profile: jack` |

Two scripts join the first attempt's in `packages/client/acceptance/`: `expire.ts` holds a large
payload's reference across its expiry, and `live.ts` records focus changes and live-region
announcements while a Session works. Activities are opened by keyboard: a Transcript sticking to
the bottom keeps moving under a pointer.

### Live Transcript: an expired payload — passes

Entry 241 of #546's Workspace held a 69,466-byte tool result. A browser opened its Activity and
showed "Load 68 KiB payload". With that browser still open, the control plane was stopped,
entries 238–248 were backdated 31 days in the store, and the control plane started; its retention
sweep expired 11 entries at 05:29:17Z. Loading the reference the browser already held then read
**expired** (410), and reopening the Activity read "expired 01:29:17 AM".

Across ten browser followers, 704 units completed while followed reached the browser at p50
0.08 s, p99 1.01 s, max 3.57 s; none over 5 s. Units finished inside a host sleep, or delivered as
backlog when a follower joined, are excluded.

### Late join and reconnect — still passes

The control plane was replaced at 05:06Z (raising the slot limit) and 05:29Z (the expiry), the
client at 05:45Z, and the whole stack was frozen for 206 s (below). No follower saw a gap or a
duplicate entry in its sequence, checked against Activity ranges.

### Open and continue — passes

- **Open without a Brief.** The browser's New Workspace form opened `merry-thistle-dwiqgsny` with
  no Brief (#500, #501). Its Session reached `harness_ready` while both slots were held by
  others, and the queue listed it as unbriefed with no slot. The first message, sent from the
  browser, became its Brief and queued it at position 1. The browser cannot tell those stages
  apart: its header read "Preparing" throughout, even when ready and briefed
  ([#589](https://github.com/openkestrel/kestrel/issues/589)); only the queue line said
  "harness ready".
- **Continue and tell the Turns apart.** #546's Workspace holds four Sessions, three from the
  Trigger and one from the operator. The browser Transcript opens each with "codex started" or
  "claude started" and closes each with "Session ended"; the header names only the current one.
- **Trailing exposes open background work.** Claude was asked to start a four-minute loop as a
  background task and answer at once. It answered "Started." at 05:08:43Z; the header read
  **Trailing** and the Session state carried the open unit "Print 24 ticks, 10 seconds apart"
  until 05:12:41Z, while the Session kept its slot. It became **Waiting** at 05:13:42Z, after the
  unit settled and Claude reported "printed 24 ticks".

### Work state: a learned pull request — passes

The first attempt planned a webhook Integration for this; #574 since made a polled GitHub App
read the App's Delivery log, so the rerun used polling. kestrel learned every pull request the
stretch opened that way: #576, #585, #586 and #587 appear on their Workspaces as "#585 open" and in the Work pane.
They stay current: #576's review push at 05:45Z moved its learned head to `d6d02dc`, matching
GitHub, without reading a push or a runtime log.

### Options and continuity: lost continuity — passes

Killing a harness does not lose continuity: the supervisor restarts it and resumes the ACP
session, and the Session stayed Waiting. To lose it, OpenCode's session store was removed from
the Instance and the harness killed again at 05:31:05Z. The Session failed — "the agent's process
was lost again (Process exited with signal: 9 …)" — and the browser showed it Ended with that
reason, while the Workspace stayed open and its Instance and checkout survived. The reason
carries raw harness log lines; [#549](https://github.com/openkestrel/kestrel/issues/549) covers
structured failure evidence.

### Queue — passes

| Case | `kestrel queue` and the browser, read back to back |
| --- | --- |
| Limits and occupants | `limit 2; 2 occupied (quick-thistle-qnwcgzbh, jolly-willow-psknmsmm)`; the browser: "Slots 2/2: quick-thistle-qnwcgzbh, jolly-willow-psknmsmm" and each counted Instance by name (#503). |
| Several ready, FIFO | `1 agile-robin ready`, `2 jolly-wren behind agile-robin`, `3 amber-juniper behind agile-robin, jolly-wren`; the browser "Queued #1", "#2", "#3". |
| Dependency wait | `lively-yarrow … waits on quick-thistle-qnwcgzbh`, unnumbered; the browser "waiting on quick-thistle-qnwcgzbh". |
| Distinct other reasons | the profile hold (`the Subscription Profile jack is held by quick-thistle`), the live-Instance cap (`at the limit of 3 live Instances`), and an Instance being archived to make room. |
| Original place after the blocker succeeds | B, X (depending on B), Y1 and Y2 were enqueued in that order with every slot held. When B answered, Y1 took its slot; B was ended at 05:25:07Z, and X became `1 … ready` ahead of Y2's `2 … behind ember-acorn` in both Clients. |
| Waiting Sessions | listed apart from the queue in both. |
| A read archives nothing | the Instance list and the Instance containers were identical before and after `queue.ts`. |

No estimated start time appears. Two dependents were refused a start when their blocker failed
(lost to the frozen stack, then to Codex's usage limit): each became unreachable and named it.
The check passes with a caveat: at the Instance limit, archiving an idle Instance to admit a
dependent ended its Waiting blocker as succeeded and so released the dependent before the blocker's
work was done ([#590](https://github.com/openkestrel/kestrel/issues/590)). The original-place case
above was therefore run with the Instance limit raised out of the way.

### Accessibility and narrow screens — passes after a fix

At `4d0bfa2`, axe on live Workspaces (`axe.ts`, 68 audits) found no critical violation; #504's
fixes hold. It found 15 **serious** `scrollable-region-focusable` violations #504 did not reach:
the Diff and Files tabs' `<pre>`, and the Transcript's scroll container whenever it overflows with
nothing focusable inside (a Session that ended without a Turn). This branch fixes both: the Work
tabs use `ScrollablePre`, the Transcript's scroller takes a `tabindex`, and two e2e tests cover
them. Chromium lets a keyboard into any overflowing scroller on its own, so the Diff test asserts
axe's rule rather than a key press. With the fix, 136 live audits at 1280 and 375 px, every
Activity expanded, found no serious or critical violation and no horizontal page scroll; the
moderate `landmark-one-main`, `page-has-heading-one` and `region` remain
([#512](https://github.com/openkestrel/kestrel/issues/512), now
[#587](https://github.com/openkestrel/kestrel/pull/587)).

The keyboard-only path *find a running Session → follow it → open a diff → take a turn* completed
on live data with the fix: 142 key presses at 1280 px and 99 at 375 px.

Live updates neither steal focus nor announce every tool update. `live.ts` held a draft in the
composer for three minutes on each of two working Claude Sessions, which streamed 62 state
updates, 14 tool status changes and 16 Transcript entries. Focus never moved, the draft survived,
and the polite regions announced once: a narration line. The Conversation log is `aria-live="off"`.

VoiceOver passes at 1280 and 375 px on `crisp-acorn`'s live Transcript, driven by guidepup
against headed Chromium with the operator listening. Headings read in order (the Workspaces,
Workspace and Work panes at desktop width, the Workspace alone at 375 px), the Transcript reads
as one article per message, and an Activity reads as "Activity 112–112, 1 tool, 382ms, read ·
completed, collapsed button" and expands from VO-Space. A message posted from the composer was
answered by the Muse `builder`, and the operator heard "builder: kestrel heard you", "Session
trailing" and "Session waiting" announced politely while focus and a second draft stayed in the
composer; the answer's narration lines were announced too, and no tool update was. VoiceOver's
landmark command found nothing from the browser chrome, which leaves the missing `main` landmark
to [#512](https://github.com/openkestrel/kestrel/issues/512). guidepup's `last phrase` captures
no announcement, even a calibration one, so the announcements are the operator's ears and the
client's polite regions, logged alongside.

### Leases across a host sleep

#505's fix holds under real sleep. The host slept 33 times between 05:51 and 11:57Z (357
minutes, the longest 32) with Claude and OpenCode Sessions working; the control plane recorded
each gap, and after 05:00Z its log holds no supervisor giving a Session up and no lease
expiring. One Claude Session failed at 10:07Z on "API Error:
Connection lost mid-response", Claude's own error as the host woke, and kestrel reported it as
that.

A stronger simulation does fail Sessions. `docker pause` on every container of the stack for
206 s (04:54:39–04:58:05Z) froze the processes while their clocks ran: on resume two supervisors
let their harnesses go, having not reached the link for longer than the lease plus margin
(`crates/kestrel-supervisor/src/lib.rs`), and the control plane's sweep failed both Sessions 30 s
later with "the supervisor came back after the control plane was not running from 04:54:39 to
04:58:05". The failure explains itself in the browser; no log was needed. The supervisor counts
its own time unanswered, and a frozen container's clock runs on while a suspended host's evidently
does not, so real sleep — the case #505 reported — holds. This records the limit rather than a
blocker.

### The dogfood stretch

Compressed again, with the operator's agreement, to one run on 6 October, and followed by
scripted browsers. Tickets blocked by open work were not taken unless stacked on their blocker's
branch: #547 and #558 were dispatched on top of #546's pull request. Ten Delegated Sessions came
from the `delegated` Trigger by dispatch and worked while a browser followed them. An eleventh,
`brisk-meadow`, failed before doing any work and is not counted. #543's follower hit a Playwright
timeout as it started (the host was sleeping) and rejoined at 11:57Z, mid-Session.

| Enqueued (UTC) | Issue | Harness and model | Workspace | Session | Outcome |
| --- | --- | --- | --- | --- | --- |
| 04:39 | [#546](https://github.com/openkestrel/kestrel/issues/546) | Codex, `gpt-6-astra` | `silver-summit-pzgpgnvy` | `quick-thistle-qnwcgzbh` | working; failed by the frozen stack |
| 04:39 | [#555](https://github.com/openkestrel/kestrel/issues/555) | OpenCode, Muse 1.3 | `jolly-maple-qsuhvmcl` | `jolly-willow-psknmsmm` | opened [#576](https://github.com/openkestrel/kestrel/pull/576), CI green; ended when its Instance was archived to make room |
| 04:39 | [#557](https://github.com/openkestrel/kestrel/issues/557) | OpenCode, Muse 1.3 | `crisp-acorn-qshbkhbv` | `keen-raven-vlfytdww` | waited on the Instance cap; opened [#586](https://github.com/openkestrel/kestrel/pull/586), CI green |
| 04:39 | [#512](https://github.com/openkestrel/kestrel/issues/512) | OpenCode, Muse 1.3 | `merry-heron-inluhsno` | `agile-robin-oalvypnx` | committed but could not push; a follow-up Turn had it run `gh auth setup-git` and open [#587](https://github.com/openkestrel/kestrel/pull/587) |
| 05:00 | #546 | Codex, `gpt-6-astra` | `silver-summit-pzgpgnvy` | `jolly-wren-jmmhzudn` | opened [#585](https://github.com/openkestrel/kestrel/pull/585); failed on Codex's usage limit while reading its CI |
| 05:23 | [#547](https://github.com/openkestrel/kestrel/issues/547) | Claude, default | `crisp-badger-bvnfsdza` | `grand-thistle-lrtknqed` | stacked on #585; working through the host sleeps |
| 05:23 | [#558](https://github.com/openkestrel/kestrel/issues/558) | Claude, default | `quick-fox-bbyzujzk` | `keen-otter-mjjgwvvp` | stacked on #585; failed on Claude's "Connection lost mid-response" |
| 05:38 | #546 | Codex (Claude asked) | `silver-summit-pzgpgnvy` | `brisk-meadow-ymtbhsol` | dispatched with `--agent claude`, ran Codex, failed at once on the usage limit ([#588](https://github.com/openkestrel/kestrel/issues/588)) |
| 05:38 | #555 | OpenCode, Muse 1.3 | `jolly-maple-qsuhvmcl` | `clever-owl-adhgookv` | ran `/code-review` on #576 and pushed `d6d02dc`; CI green |
| 06:21 | [#543](https://github.com/openkestrel/kestrel/issues/543) | OpenCode, Muse 1.3 | `silver-kite-xfbhmvwg` | `happy-badger-emvjmxpc` | working through the host sleeps |
| 12:08 | #558 | Claude, default | `quick-fox-bbyzujzk` | `proud-willow-rsgjuznf` | continued `keen-otter`'s checkout; working |

Outcomes marked working were still working when this was written.

An operator Session on #546's Workspace (`hidden-cedar-qetyvldd`, Claude) fixes #585's CI after
#588 set the Trigger's continuation aside; it and the scripted cases' Sessions are not counted.

Observations:

- **Nothing needed a reach past kestrel to understand a running Session.** Both stack-freeze
  failures, the two usage-limit failures and the API failure each read in the browser with their
  cause. `docker exec` was used only to break things on purpose.
- **Codex's coverage is partial.** Its subscription window ran out in under an hour and ended two
  Sessions with its reset time; the third never started work. Codex was followed working for
  about an hour across two Sessions, which opened #585; the first of them was ended by the
  operator's stack freeze, not by Codex.
- **Muse 1.3 (free) did real work**: three pull requests with green CI and a review pass, but on
  #512 it took "no GitHub auth" at face value instead of running `gh auth setup-git`.
- **Claude Turns stalled for hours** while the host slept, then resumed without loss.
- One dispatch after the sleeps was refused with "github answered 422 Unprocessable Entity for an
  installation token"; the same dispatch a minute later succeeded.

### Findings

- [#588](https://github.com/openkestrel/kestrel/issues/588): a dispatch's, label's or command's
  Agent is dropped when the correlation finds an open Workspace.
- [#589](https://github.com/openkestrel/kestrel/issues/589): an unbriefed Session reads
  Preparing after its harness is ready and while its Brief waits.
- [#590](https://github.com/openkestrel/kestrel/issues/590): archiving a Waiting Session's
  Instance to make room releases its dependents.
- On [#511](https://github.com/openkestrel/kestrel/issues/511): a dependency wait reads "Waiting:
  waiting on …"; "Session ended" does not say how.

### Result

**The gate is met**, and the `ROADMAP.md` marker moves to `0.4`. At `4d0bfa2` it stopped on 15
serious `scrollable-region-focusable` violations. With this branch's fix, no serious or critical
violation remains in 136 live audits and CI's axe suite is green, every other scripted check
passes, with the caveats #589 and #590 record, the VoiceOver pass passes at 375 px and at desktop
width, and the stretch needed no reach past kestrel.

The five-day span of the stretch was waived by the operator, as in the first attempt, and Codex's
share of it is thin.
