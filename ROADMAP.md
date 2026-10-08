# Roadmap

> **We are here: `0.4`, kestrel starts from one command.** `0.3` is closed: kestrel's work is
> joinable mid-flight. A person opens a running Workspace in the browser or the CLI, reads a live
> Transcript that summarizes each Activity and expands it on demand, takes a turn alongside others,
> and sees the queue, its Instances, the branch and the pull request as current state. Triggers
> continue an open Workspace instead of opening a second, and a Session's lease survives a sleeping
> host. The acceptance runs and what they found are [`docs/acceptance-0.1.md`](docs/acceptance-0.1.md),
> [`docs/acceptance-0.2.md`](docs/acceptance-0.2.md) and
> [`docs/acceptance-0.3.md`](docs/acceptance-0.3.md). The repo holds the vocabulary in
> [`GLOSSARY.md`](GLOSSARY.md) and the direction in [`README.md`](README.md). Starting kestrel still
> means reading a guide.

Eight rungs from an empty repository to v1, and no dates. Each is a `0.N` release, and each is named
by a class of kestrel's own work rather than by a feature list, because the ladder is walked by
kestrel building kestrel: a rung nobody can dogfood does not belong on it. [`README.md`](README.md)
says what v1 means and what the twelve capabilities are. This document says the order they arrive in,
and how you tell that one has landed.

## How to read the ladder

**A rung closes when its class of work is the default path, not when kestrel has done it once.**
"kestrel opened a PR" and "kestrel opens the PRs" are separated by roughly everything that makes
software real, and only the second one is a test. The consequence is accepted rather than regretted:
rungs are slow and few, which is the right shape for a ladder whose top is a stability lock.

**There is no rung on which a workspace is allowed to be ephemeral.** A workspace's whole truth lives in
storage and its transcript from the first commit, so durability is not a feature added to a workspace
later — it is what a workspace *is*. An in-memory first rung would not be a stub you replace, it would
be a different program. That is why there is no "point of no return" marked further up the ladder:
the point of no return is rung one.

**Depth first, breadth once.** The generic CloudEvents endpoint accepts trigger events from any
producer at `0.1`, so breadth arrives on the inbound path from the first rung. The named surfaces
still wait until `0.7`: each needs its adapter and outbound half, and all seven external surfaces
must round-trip. A schedule has no external recipient, so its outcome is visible in kestrel. This
adapter work is delayed until the durable workspace model has stopped moving underneath it. The
order avoids coupling those adapters to the changing work model; it does not ask a team to wait for
an adapter before kestrel can receive its events.

**`Organization` is in every durable record from the first migration**, while multi-tenancy is a
`0.8` capability. The boundary is ruinous to introduce late and cheap to carry early, so it is added
at rung one and stays invisible until the rung that makes it mean something. Multi-tenancy could not
have been a rung of its own in any case: kestrel developing kestrel is one organization, so there is
no dogfooding test for the second one.

**Moving the marker at the top of this file is part of each rung's definition of done.** There are no
per-rung checkboxes. A version number is the one claim about progress you can verify against the
release page in five seconds, and half-ticked rungs would re-litigate once-versus-default-path on
every read.

## The ladder

|     | Rung                                   | The class of kestrel's own work                              | v1 capabilities                                                                            |
| --- | -------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| 0.1 | **kestrel opens its own PRs**          | issues labelled `ready-for-agent` are worked by kestrel, not by you in a terminal | trigger ingestion (GitHub and generic CloudEvents), isolated execution, model choice, persistent workspaces, storage (SQLite) |
| 0.2 | **kestrel works the backlog**          | many issues at once; you stop being the queue                 | scheduling                                                                                   |
| 0.3 | **kestrel's work is joinable mid-flight** | you pick up a running workspace instead of reading a finished one | multiplayer, browser Client, queue and work visibility                                        |
| 0.4 | **kestrel starts from one command** | you go from `docker compose up` to a working Session without reading a guide | guided first run in the CLI and the browser, harness sign-in, published images |
| 0.5 | **kestrel asks before it acts**        | kestrel does work you would not have let it do unsupervised   | governance, operator identity, managed Skills, MCP, Trigger inspection                      |
| 0.6 | **kestrel runs multi-step work**       | classes of work that are a sequence, not a single session     | workflows and Campaign operations                                                             |
| 0.7 | **kestrel meets the team where it works** | integrations return outcomes where work began               | seven external surfaces with outcomes, plus schedule                                          |
| 0.8 | **kestrel runs where you run**         | kestrel develops itself on infrastructure that is not your laptop | pluggable storage, the rule of two, the eight targets                                    |
| —   | **v1**                                 | the lock                                                      | —                                                                                            |

### 0.1 — kestrel opens its own PRs

An inbound CloudEvent reaches the generic endpoint, a trigger matches it, a workspace opens, a session is
scheduled on an Instance, and a pull request lands on this repository. The class of work is issues
labelled `ready-for-agent`: they are worked by kestrel rather than by a person in a terminal, and the
rung closes when that is how they are worked by default. GitHub is the dogfood case, not the
ingestion boundary: anything that can POST a CloudEvent can start work here with no adapter or
integration.

**This is the biggest rung on the ladder, and it is irreducible.** A single triggered pull request
needs the trigger path, the scheduler, the compute contract, the harness boundary, durable
storage, the transcript, and SQLite, all at once. Every split anyone has proposed produces a rung on
which the human is still the scheduler, which is the exact state this project exists to reject.
Everything above this rung is addition; this one is creation.

Some of what lands here is invisible on the day it ships and impossible to add cheaply afterwards:
the `Organization` column on every durable record, the transcript's entry granularity and the fact
that state is held as current values rather than replayed out of history, the `sealed` state on a
workspace, the session-held lease, and the bounded-window-plus-paging transcript read. That last one is
needed here rather than at `0.3`, because a resumed session reads the transcript for context, and the
need arrives before any human has ever joined a workspace. The reason this rung first gave for the read
— compute disposable from the first commit — is history: ADR-0018 gave it up, and an Instance now
lives until its workspace seals
([ADR-0018](docs/adr/0018-an-instance-lives-until-its-workspace-seals.md)). The read keeps its seat for
a resumed session whose box is not there to resume into — one archived once its work was safely pushed,
or lost outright — where a fresh Instance is provisioned and restored from the remote, and the
transcript is what the session reads for context. The lease is here rather than at `0.2` for a
neighbouring reason: without one, an Instance that dies mid-session leaves its session active forever,
holding the workspace's one active-session slot, so the workspace
never seals — and a rung that promises an interrupted session ends with an explicit exit status cannot
ship a workspace that wedges permanently. Also settled here by omission: what happens inside a session is
the session's business, not the workspace's, and gets no promise and no name.

### 0.2 — kestrel works the backlog

Many issues at once, and you stop being the queue. The rung is scheduling: a ledger of queued sessions
with dependency edges between them, claiming, at-most-once dispatch, and a deterministic FIFO ready
order. The lease itself landed at `0.1`; what arrives here is the graph its expiry unblocks, and the
rule that expiry fails a session rather than re-dispatching it. Workspaces also start sealing themselves
here, on idle expiry, riding the same timer sweep that reaps leases.

Durability arrives with it, because "works the backlog" is false while a session can take its own output
down with it. An Instance now outlives its sessions, and is never reaped while it holds work that exists
nowhere else: the supervisor closes every turn by reporting the checkout's git state, kestrel judges
recovery from what is committed, pushed, uncommitted and untracked — never from an agent's assertion
— and a workspace's next session finds the same Instance and the checkout exactly as the last session left it,
so a follow-up can commit and push what a finished turn left behind. kestrel also declares the branch
its sessions work on, which is what makes that restore possible and, at `0.3`, the pull request learnable
([ADR-0018](docs/adr/0018-an-instance-lives-until-its-workspace-seals.md),
[ADR-0019](docs/adr/0019-kestrel-declares-the-branch-and-learns-the-pull-request.md)). A cap on live
Instances at the Organization bounds what keeping the box costs.

Concurrency arrives with it, and it lives **across** workspaces and never within one: at most one session
is active in a workspace, always. That is the project's most surprising design decision, and it is what
lets a backlog be worked in parallel without turn-taking inside a workspace becoming a lock problem.

**This is the weakest boundary on the ladder, and it is flagged rather than defended.** A rung that
turns out easy is a better outcome than a rung quietly hiding four subsystems, so the boundary stays
where it is — but "kestrel worked an issue" and "kestrel worked this week's issues" are different
claims, and the second one is where a helper becomes a factory.

### 0.3 — kestrel's work is joinable mid-flight

You pick up a running workspace instead of reading a finished one. Multiplayer is one uniform promise
designed to the weakest transport kestrel supports, so every deployment gets the same guarantees and
the faster ones are only faster.

The rung is bigger than the read `0.1` built, and this is where the transcript catches up. It is
rewritten into three kinds under one order and one cursor — shared state, narration, and detail — and
a read names the kinds it wants. A person joining late gets shared state plus a summary of each
Activity; expanding one reads its narration and detail. Each completed unit reaches the Transcript
as it completes rather than being drained at session end, and running tools appear as transient
Session state, so an agent stuck thrashing is visible while it thrashes
([ADR-0020](docs/adr/0020-the-transcript-records-what-the-runtime-emits-in-kinds.md),
[ADR-0037](docs/adr/0037-a-transcript-read-summarizes-the-activity-it-omits.md)).
Narration and detail content lasts 30 days from append; expiry leaves a cursor-preserving marker,
and large payloads live behind references ([ADR-0033](docs/adr/0033-expire-transcript-detail-in-place.md)).

Learning the pull request lands here too, as shared state rather than session detail: the branch `0.2`
declared correlates the `pull_request` event the integration already delivers, so a joining human
knows a pull request exists without the tool calls that pushed it
([ADR-0019](docs/adr/0019-kestrel-declares-the-branch-and-learns-the-pull-request.md)). Joining is a
second consumer of the same bounded window and cursor a resuming session already uses. A connection is
never the unit of workspace continuity: a follow resumes its Transcript cursor after a disconnect,
while Organization change notices prompt current-state reads again. Presence travels with a follow,
is held only in memory, and never gates anything.

**The browser Client is where joining surfaces.** It is a static Client served by the loopback
operator listener the CLI uses, reachable through a tunnel but not published as a public listener. It
opens and follows Workspaces, shows the live Transcript, diffs and read-only live files, and lets a
person take a turn. Opening a Workspace without a Brief starts its first Session unbriefed, ready for
the first message to become its Brief ([ADR-0038](docs/adr/0038-a-session-may-start-before-its-brief.md)).
The Client shows the declared branch, learned pull request, and Instance work, distinguishing pushed,
committed, staged, changed and untracked work. The Instance's supervisor reports that work live over
its link ([ADR-0039](docs/adr/0039-the-supervisor-lives-with-its-instance.md)). It shows the
requested and effective model, and reports ACP conversation continuity separately from the durable
kestrel Workspace: losing harness context must not look like successful resume. The queue is one
operator snapshot derived at read from the rules dispatch applies: ready Sessions numbered in FIFO
order, each other
queued Session naming what it waits on, the Active-Work Slot and live Instance limits with what
occupies them, and Waiting Sessions apart from the queue. Reading it never moves dispatch, and
nothing in it estimates a start time.

A person joins under a declared name when they first take a turn. They can interrupt a working Turn
without ending its Session, and see, edit or withdraw Held Messages until the agent takes them. A
Turn that has answered while its agent's work continues leaves the Session trailing: its open work
stays visible and holds an Active-Work Slot until it settles; it can take another Turn immediately
([ADR-0040](docs/adr/0040-a-session-trails-its-answer-while-its-work-runs.md)).

Between Turns a person changes a Session's options: its model, mode, thought level and whatever else
its harness offers, warned when a change costs the prompt cache. A Trigger declares them by what they
are for. The Session shows its harness's title, its usage as it happens, and the commands its harness
offers, and a Held Message beginning with a command becomes its own Turn
([ADR-0041](docs/adr/0041-a-sessions-options-are-its-harnesss-config-options.md)).

Feedback is prompt, work state is clear, and the view is accessible and responsive. The same event
stream and current-state reads serve Clients through the operator boundary, while the browser also
uses change notices that the CLI does not consume at this rung
([ADR-0015](docs/adr/0015-the-cli-is-a-client-not-a-role.md),
[ADR-0035](docs/adr/0035-organization-change-notices-and-workspace-presence.md),
[ADR-0036](docs/adr/0036-the-browser-client-shares-the-loopback-operator-origin.md)).

### 0.4 — kestrel starts from one command

You go from `docker compose up` to a working Session without reading a guide. The first person to
drive kestrel from a fresh build met a stale volume reported only as "unhealthy", a certificate to
trust by hand, a blank page, a subscription token passed to a harness that cannot use it, a harness
the image did not carry, and a login read from standard input until Ctrl-D. Each was a dead end
behind an error that named none of it, and the documents that would have helped were spread across
four files. A rung nobody can start does not get dogfooded.

**`docker compose up` is the only step.** The release's pull-only `compose.yaml` pins published
images by multiarch manifest digest; main's development Compose keeps source builds. The browser
Client is served over plain HTTP on loopback, so there is no
certificate to trust: a tab holds one event stream and subscribes over requests
([ADR-0045](docs/adr/0045-a-browser-tab-holds-one-stream-and-subscribes-over-requests.md)). A control
plane that cannot start says why in one line, and the Client says it is not running rather than
waiting for it to be healthy.

Image capability checks resolve the configured image again before reporting availability or
accepting work, caching labels by immutable image identity. Missing images and daemon inspection
failures leave the control plane reachable and report unavailable, without a stale positive result
or an automatic pull. An incompatible store names the cause and an explicitly destructive reset
command; startup never deletes saved data. The operator API root names the configured Client URL
([first-run images decision](https://github.com/openkestrel/kestrel/issues/532)).

The browser polls sign-in relay progress so each tab retains one SSE connection. Transcript
subscriptions resume from their own cursors; Organization notices cause a refetch after reconnect.
Plain-HTTP verification covers five followed tabs with ordinary requests, respecting ADR-0045's
accepted connection limit ([first-run images decision](https://github.com/openkestrel/kestrel/issues/532)).

**Every surface guides, and none is required.** The operator API is the one surface; the CLI and the
browser are each complete Clients of it, and each walks a person through the same steps: name the
**Operator**, choose a harness and sign in, connect GitHub and choose a repository, write a Brief
([ADR-0049](docs/adr/0049-an-install-has-one-operator-and-by-default-one-organization.md)). The
control plane answers what is missing and what fixes it, and both Clients render that answer. In the
CLI, `kestrel start` asks for what it cannot infer and every prompt has a flag; errors offer a
typed next step that each Client renders as a repair, inspection or retry
([ADR-0052](docs/adr/0052-a-refusal-carries-its-next-steps.md)), and the shell completes names.

The browser asks one focused question at a time and ends with a review before starting the first
Session. Saving the Operator's name is one prerequisite, not the end of setup: returning before the
first confirmed start succeeds resumes the remaining steps. Later gaps use contextual fix-it states
and settings ([browser setup decision](https://github.com/openkestrel/kestrel/issues/491)).

The confirmed start transaction completes initial setup when it commits the Workspace and enqueued
Session, with completion durable across restarts. Later execution failures use contextual repairs.
A partial preview returns saved prerequisites, resolved values and all established gaps without
writes or secrets. Start creates missing Project/Agent declarations and reuses matching ones;
conflicts require explicit declaration/settings changes before another review
([shared start decision](https://github.com/openkestrel/kestrel/issues/535)).

Browser setup and Agent settings use the harness default or an explicit model-id override before
ACP options exist; searchable model choices come from a live Session's offered values. Setup never
starts a hidden Session to discover them. The browser build slice owns the shared AI Elements Model
Selector and its live Session integration; the workbench places it in the refined composer. Provider
marks are bundled locally with attribution and a text fallback, without a runtime `models.dev`
dependency. Geist and Geist Mono land once through the browser setup slice, for both specs
([browser setup/settings specification](https://github.com/openkestrel/kestrel/issues/527)).

The CLI asks "What should the Session do?" and keeps `--brief` for its initial prompt. A start
without a terminal names all missing inputs it can establish and requires `--yes` before any
write. Its review distinguishes already-saved setup from the start plan; declining creates no
Workspace or Session and retains setup. Results go to stdout, prompts, progress and errors to
stderr, and redirected output uses no colour or terminal animations
([CLI first-run decision](https://github.com/openkestrel/kestrel/issues/493)).

**Signing in is kestrel's job.** kestrel ships a catalogue of how each harness signs in
([ADR-0046](docs/adr/0046-kestrel-ships-a-catalogue-of-how-each-harness-signs-in.md)), relays a
person's own subscription sign-in to whichever Client they are using, including a terminal on a
remote machine ([ADR-0047](docs/adr/0047-kestrel-relays-a-persons-own-sign-in.md)), and checks each
sign-in without automatically spending model usage: supported provider checks establish credential
acceptance, while an optional model test or actual Session establishes harness/model evidence. The
default image carries every catalogued harness and
says so in a label, so kestrel never offers one it cannot run
([ADR-0048](docs/adr/0048-the-environment-image-carries-every-catalogued-harness.md)).

**A tag prepares a release; acceptance publishes it.** A reviewed release PR sets the shared
product version and curated changelog, and the maintainer tags its merged commit. Automated release
checks, including Conformance, pass before preparing draft-release artifacts: four native-built
Linux amd64/arm64 images (`kestrel`, `kestrel-client`, `kestrel-env`, `kestrel-dev`), the `kestrel` CLI
for Linux and macOS on both architectures, and generated pull-only Compose. The control-plane
image carries the CLI too. Fresh-machine acceptance runs once from the browser alone and once
from the CLI alone over SSH against the same tagged `0.4.x` candidate, after the `0.3` gate passes.
Only success authorizes publication, moving `latest` and advancing the ROADMAP marker. Published
version tags and artifacts stay immutable; a failed candidate remains draft and fixes land in a
reviewed patch release. Main uses `main` and commit-SHA image tags
([ADR-0053](docs/adr/0053-a-tag-prepares-a-release-acceptance-publishes-it.md)).

The CLI distribution includes archives, checksums and a version-selected shell installer, with
documented retrieval of the exact draft assets for acceptance and a tested Linux glibc 2.35 floor.
Release checks emit the candidate's checked commit/version identity; native image publication emits
its platform and multiarch manifest digests. The generated Compose and all CLI assets join those
records into one verified candidate inventory. Infrastructure retries may fill missing artifacts
only after verifying existing immutable content; disagreement stops preparation rather than
replacing a published artifact.

Promotion consumes that exact inventory and the successful acceptance evidence. It preflights all
references, moves and verifies all four `latest` aliases, then publishes the GitHub release. A
partial failure remains visibly incomplete and resumable for the same approved inventory, with no
automatic rollback, rebuild or ROADMAP advancement. Promotions are serialized and an older candidate
cannot silently replace a newer accepted release. The final operator acceptance ticket owns both
journeys, the `0.3` prerequisite and the marker; preparing artifacts never waits on those journeys
([release build specification](https://github.com/openkestrel/kestrel/issues/545)).

`kestrel login --token` imports an existing token or key through hidden input or stdin;
`kestrel login --file PATH` imports a login file. Both name their harness and validate the input;
interactive login prompts for a missing method, and non-interactive login requires one when
ambiguous. `kestrel sign-in test` tests saved material only when explicitly requested.
The test uses only the selected sign-in, never silently falls back to an Organization key, and
reports the model used. OpenCode requires an explicit provider/model; Codex tests share its Profile's
serialized credential use and report busy instead of racing a Session's login refresh.
If a provider validation service is unavailable, the saved sign-in remains unchecked and the Client reports
that partial result. GitHub uses `kestrel integration github connect` and always acts as an App
([ADR-0028](docs/adr/0028-an-integration-lends-a-run-its-identity.md)); registering an existing App
is the fallback for the manifest flow, never a personal access token.

GitHub setup creates and installs the App before asking which repository to use. Its temporary
flow lists accessible repositories with their default branches; confirming one creates a
single-repository Integration. Pending selection resumes across a control-plane restart within
the flow's one-hour lifetime. Completion, cancellation and expiry erase temporary local secrets;
they never delete or uninstall the external App. Safe completion state lets either Client observe
the result without creating another Integration. Browser setup/settings and the CLI use the same
flow, repository validation and lifecycle contracts
([GitHub and repository build split](https://github.com/openkestrel/kestrel/issues/534)).

**Polish covers routine use, not only setup.** Failed declaration and queue reads say they failed
and offer recovery; an unavailable Transcript never looks like empty history. Recovery actions,
help, branch and People wording, keyboard navigation and moderate accessibility findings are part
of `0.4`. Integration maintenance keeps its repository, App and installation identity fixed;
name, directions, polling interval and credentials can change. Disable pauses use; retirement erases
credentials while retaining history. Projects explicitly select the Integration and Workspaces fix
that selection when they open, so replacement never silently changes existing work's authority.
App key rotation is validated and atomic locally; webhook-secret rotation is resumable across
Kestrel and GitHub. Polling continues alongside a reachable webhook, preserving its cursor
([ADR-0056](docs/adr/0056-an-integration-keeps-its-identity-through-maintenance.md)). Existing `0.3`
acceptance blockers keep their owners and must pass that gate; unverified cancellation observations
require evidence before they become feature commitments
([papercut scope decision](https://github.com/openkestrel/kestrel/issues/494)).

The workbench names open tools and Adapter Units while a Session trails, and otherwise says
“Checking that background work has settled.” It offers a new Turn immediately without promising a
completion time. If current Session observations are unavailable, it says so rather than deriving
quietness from an empty cached list. Last reported work survives disconnection, release and a
control-plane restart with its report time and originating Instance identity; it remains historical
and is never attributed to a replacement Instance. A missing report says “No work report received.”
Lease expiry is displayed as a deadline, never as the last contact or proof of availability; only
the control plane judges that a Session has ended, including ADR-0054's grace.

Outbound comments preserve the complete ordered agent-message response under “Turn response” in
`0.4`. ACP supplies no reliable final-answer distinction, so message position, wording and Turn
boundary metadata are not used to guess one. A future producer contract may select an explicitly
identified final answer; all messages still belong in the Transcript. Turn comments remain posted
at the answer, with trailing output recorded separately and existing Outcome deduplication intact
([workbench polish decision](https://github.com/openkestrel/kestrel/issues/539)).

Repository declarations share syntax validation and GitHub shorthand expansion. Unbound declarations
do not require a network probe; a selected GitHub Integration must establish repository access and
the checkout base branch before saving a new or changed binding. Failure preserves the prior
declaration. GitHub-backed checkout uses HTTPS with the App's authority, while actual checkout
success remains the supervisor's to establish
([ADR-0057](docs/adr/0057-a-repository-declaration-is-not-a-successful-checkout.md)).

**The CLI emits complete JSON.** `--json` returns an object for one resource and an array for a
collection, including `[]` when empty; following emits one JSON object per line. `--jq EXPR` uses
embedded jaq to filter that same response without installing another tool: strings print without
quotes, other values as JSON, collections are filtered as a whole and followed streams one record
at a time. Filter syntax is checked before any write; errors go to stderr and no results is success.
Field projection through `--json id,name` is removed. Users can still pipe the complete JSON to jq
([papercut scope decision](https://github.com/openkestrel/kestrel/issues/494)).

**Contributing is a second DevEx track.** On the contributor baseline's workloads and comparable
conditions, `0.4` targets a median of at most 120 seconds from an empty target to the first passing
supervisor-backed test (downloads excluded), 3 seconds for a warm repeat, and 300 seconds for an
executed Rust CI job. Each median uses three comparable runs; CI cache conditions are recorded and
reused gates are excluded. A focused test after editing one test module is also measured against
the original topology, so consolidation's rebuild cost is visible. Missing a budget keeps the
contributor slice open for investigation; timing does not become a per-run CI failure. This track
has its own validation and does not change the browser-only and SSH CLI-only operator acceptance
journeys ([contributor targets decision](https://github.com/openkestrel/kestrel/issues/495)).

The ordinary control-plane integration tests compile as one binary; heavy suites remain separately
selectable. Fixtures are independent under ordinary `cargo test`, and CI keeps nextest's
process-per-test isolation. Executable helpers are prepared once per invocation with Cargo
establishing freshness; direct `cargo test` needs no manual prebuild. Workspace code defaults to
line-table debuginfo and dependencies to none, with a documented full-debug override. Existing
nextest and caching stay; mold, sccache and cargo-hakari wait for measurements that justify them.

The test harness keeps its boot and lifecycle core. Common fixtures replace duplicated setup,
and necessary SQL fault injection and observation sit behind a small typed interface. Affected
tests migrate and replaced wrappers are removed; wholesale replacement of the operation layer
with scenario entry points is deferred. A one-page `CONTRIBUTING.md` covers pinned tools, one
passing test, narrow checks and filtering, helper preparation, target-directory isolation,
hermetic Git fixtures, the full-debug override and CI as the full gate.

### 0.5 — kestrel asks before it acts

kestrel does work you would not have let it do unsupervised: policy enforced at the execution layer
rather than by prompt, an approval that reaches a human where they already are, and an audit record
wider than the choices policy consults it on — every decision kestrel made unattended records the
inputs it was decided from and its verdict, whether or not a policy was consulted, and a governed
decision is one kind of entry among them. Deliberate deletion lands here too, along with the tombstone
that keeps a transcript gap-free and the rule that deleting a workspace removes nothing from the audit
record. A Session's mode is a governed choice: a mode that stops the agent asking permission would
otherwise act outside Policy, so Policy decides which modes an operator, a Trigger or the agent may
select, and each change is audited.

**This is the rung on which kestrel becomes usable by someone who is not the maintainer.** Below it,
kestrel acts on your repository with no approval path and no audit record: defensible for the one
person who owns it, indefensible for anyone else. The browser Client already makes live work usable
at `0.3`; `0.5` makes shared operation governable, and `0.7` carries it into the team's other tools.

Approvals are proven here over GitHub and the generic webhook rather than over Slack. Building a chat
adapter against a governance model that this rung is still inventing is the coupling depth-first
exists to avoid, and the generic webhook gives approvals a second surface without a second
integration.

**The Client answers Approvals and Questions interactively** and becomes the place for routine
administration. The agent half already exists — the supervisor answers ACP's permission request,
today by allowing once — so this rung adds the path outward to the person watching. OIDC and SAML
authenticate browser and remote CLI operators, identity-provider groups map to Organization access
grants, and a local administrator path bootstraps a single-machine install. Policy authorizes Workspace
discovery, read and join separately; posworkspace of a link grants none of them. Approvals arriving
through Integrations continue to authorize verified external identifiers without requiring a kestrel
account. This fills the operator identity slot [ADR-0015](docs/adr/0015-the-cli-is-a-client-not-a-role.md)
reserved for `0.5`, while retaining the external identity decision in
[issue 16](https://github.com/jtmthf/kestrel/issues/16).

**Operators can inspect decisions as well as answer them.** A Trigger dry-run evaluates a sample or
recorded Event without starting a Workspace or changing what will fire next. It shows matching,
correlation, rendered Brief and the reviewed Agent, Project, model and Policy that would supply
authority. The historical Event trace includes nonmatches, Firings and ignored evaluations, using
the inputs and verdict recorded at the time rather than today's configuration. The Audit Record is
searchable by time, Workspace or Session, actor, attempted operation, Policy and verdict; entries expose
decision inputs and the Policy snapshot and link to their causes and outcomes. Authorized readers
can page through a stable machine-readable export, with secrets redacted from routine output. An
authorized operator may open an interactive Instance shell under Policy, and its operations are
audited rather than bypassing the control plane.

**Skills and MCP become managed capabilities here.** An Organization catalog versions Skills selected
by Projects and Agents. kestrel stages them in each Harness's filesystem convention; the
harness decides whether to load one or run it as a command. A repository copy wins by name unless
Policy denies it; the effective source is visible, staging never overwrites repository files, and
failure to stage a selected Skill fails the Session visibly. Each Session retains the exact managed versions
delivered; agent-advertised use is recorded as a claim, with no inference when the agent reports
none. Kestrel's own MCP tools start with
pending Events ([issue 90](https://github.com/jtmthf/kestrel/issues/90)), and external MCP servers
can be selected from an Organization catalog by Projects and Agents. Stdio is the baseline; HTTP
requires harness capability advertisement, SSE remains a compatibility path, and unsupported
transport fails visibly. Kestrel mediates external tool calls through Policy and the Audit Record,
supplying per-server, per-Session credentials without ambient harness secrets. Event data and unreviewed
repository MCP configuration cannot select tool authority.

### 0.6 — kestrel runs multi-step work

Classes of work that are a sequence rather than a single session. A workflow declares a roster of agents
that may be enqueued plus the caps and tolerances that bound one enactment of it; each firing of a
trigger that names one begins a campaign, and the sequence is grown at runtime by sessions enqueueing
further workspaces. Campaigns carry a concurrency cap, a spend cap, and the scope a cancellation
applies to — and cancelling terminates active sessions rather than draining, because a cap that stops
only queued work does not bind.

A handoff is an enqueue and never a message. There is no coordination bus: what kestrel delivers is
ordering and once-only dispatch, the brief passes as the new workspace's first transcript entry — the
same shape a trigger already has with an event — and artifacts pass through the workspace on the
branch kestrel declared for the workspace, a name known before any session starts and a source of facts
kestrel receives from the supervisor and the integration rather than inventing for itself, since it
runs no git command ([ADR-0019](docs/adr/0019-kestrel-declares-the-branch-and-learns-the-pull-request.md)).
`0.1` already writes those first entries; this rung only adds a second kind of writer.

**Governance precedes workflows as a constraint, not a preference.** A workflow's approval step is
governance machinery, so `0.5` has to land first. The README lists the two as independent
capabilities; the ladder cannot.

The Client makes a Campaign inspectable as a graph of child Workspaces and dependency edges, with
status, blocked or unreachable reasons, spend and navigation into each Workspace. Authorized people
can post follow-ups, pause and resume a Campaign, or cancel it. Pause stops new dispatch while active
Sessions finish; cancel terminates active Sessions. There is no separate skip or rewire operation.

### 0.7 — kestrel meets the team where it works

GitHub, Slack, Linear, Jira, Microsoft Teams, GitLab and the generic webhook round-trip, so an
external surface that started a Workspace receives its outcome there. GitLab is a second source-code
host: its repository and issue Events can start work, and its merge request and result are correlated
and reported there. Jira work items and Teams conversations can start work and receive results in
their own context. Each Integration declares its inbound and outbound capabilities; they share the
Event, Trigger, Workspace and Outcome model without pretending to have identical native operations.
Inbound CloudEvents have worked since `0.1`; this rung adds the named adapters and outbound paths.

Multi-repository GitHub installations are explicit work at this rung: settle shared credential
ownership, repository bindings and authority routing through
[Define the Integration domain and architecture](https://github.com/openkestrel/kestrel/issues/140).
`0.4` retains one repository per Integration while making its maintenance and replacement deliberate.

Scheduled Triggers join the same Event and Firing path, with intervals and time-zone-aware calendar
recurrence, including the [cron acceptance slice](https://github.com/jtmthf/kestrel/issues/198).
A schedule has no external surface to reply to, so its outcome is visible in the Client and CLI.
Bitbucket and Azure DevOps remain possible additions through the generic Integration seam, rather
than named v1 guarantees.

### 0.8 — kestrel runs where you run

kestrel develops itself on infrastructure that is not your laptop. Postgres joins SQLite, and every
pluggable layer ships its second real implementation — the rule of two — which is the same work as
standing the eight deployment targets up, since both are the question of whether a contract survives
being driven twice. Multi-tenancy becomes a capability here, on the `Organization` boundary that has
been in every record since rung one. The Environment arrives as the declaration `0.1` was always
pointing at: an image, a size, and the setup layered over them, named once and selected per session,
which retires the per-language images that carried the interim, with the archive timeout configured
on it ([ADR-0017](docs/adr/0017-the-environment-is-declared-the-instance-is-provisioned.md)). The
advisory idle hint arrives with it: kestrel tells a compute backend when an Instance is idle and lets
the backend's economics do the rest — a hint that is information rather than a capability, because a
backend that ignores it is expensive, never degraded
([ADR-0018](docs/adr/0018-an-instance-lives-until-its-workspace-seals.md)).

The eight targets are the top of the ladder and not the on-ramp. At kestrel's capability floor you
are committing to a server on every one of them — you are renting it monthly instead of running it —
so the free path is docker-compose on your own machine, and it is a first-class one rather than a
demo mode.

## v1 — the lock

v1 is not a separate implementation rung. It is the stability lock applied after the `0.8` product
floor is complete: the project commits to no breaking changes until v2, with semver on its public
API, workspace-preserving migrations, a documented upgrade path and a deprecation policy. Kestrel's
ACP client has been proven against two agents of different lineages. The browser Client handles
routine work and administration after initial installation, while the CLI remains available for
scripting and power use. The twelve capabilities in the README are the content of the freeze; the
lock is the day this project is willing to stop changing its mind. The `0.x` line carries real,
recommended releases, and is where people will live for a long time.

By `0.8`, kestrel should be the thing that moves the marker at the top of this file.
