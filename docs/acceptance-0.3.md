# Acceptance attempt: kestrel's work is joinable mid-flight

The `0.3` acceptance gate ([#426](https://github.com/openkestrel/kestrel/issues/426)) was run on
3–4 October 2026 at `5da5466`, on this repository's `ready-for-agent` work. **The gate is not
met.** Three scripted checks fail and two pass only in part; one critical and one serious axe
violation appear on live Workspaces; and a lapsed lease could be explained only from the control
plane's log. The `ROADMAP.md` marker stays at `0.3`; the blockers are under [Result](#result).

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
