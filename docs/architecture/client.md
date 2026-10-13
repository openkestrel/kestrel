# Clients

The `kestrel` CLI, the first Client ([ADR-0015](../adr/0015-the-cli-is-a-client-not-a-role.md)),
in `crates/kestrel-client/`, and the browser Client in `packages/client`. Both drive the
[operator boundary](operator-boundary.md).

## The Client

- **Where it points**: `KESTREL_CONTROL_PLANE`, default `http://127.0.0.1:7718`.
- **Scope is derived, never stored** ([ADR-0016](../adr/0016-scope-is-derived-never-switched.md),
  `scope.rs`): `--organization`, then `KESTREL_ORGANIZATION`, then a `.kestrel/organization` file
  found walking up from the working directory, then the only Organization that exists. Several
  Organizations and no binding is an error that lists them; none at all and no Operator is the
  control plane's naming gap, exit 78, whose step is `kestrel operator name`.
- **Bare `kestrel`** (`readiness.rs`) reads readiness for the derived scope and prints it to
  stdout: `Ready` and the established path, or `Not ready:` and the first gap, then the Client
  URL. It exits 0 when ready, 78 when blocked and 5 when unavailable. The first gap goes to
  stderr as any Diagnostic does, with its steps, and each warning follows `warning:`. `--json`
  writes the whole read to stdout. Only the heading is coloured, on a terminal with `NO_COLOR`
  unset. Several Organizations and none in scope is exit 3 before anything is read.
- **The Operator**: `kestrel operator name NAME` names or renames them and `operator show` reads
  them. Every `--as-participant` is optional and sent only when given; the Client supplies no
  name of its own.
- **Output** (`output.rs`, `typed.rs`, `view.rs`): a migrated command decodes the answer into its
  `kestrel-operator-types` value with `ControlPlane::get_as`, `get_where_as`, `post_as`, `put_as` or
  `patch_as` and declares `Columns` that read its fields; the Operator, Organization, Project,
  Agent, Event, Trigger, Credential, Profile and Integration commands, both `apply` results, scope
  resolution and `status`'s Project, Agent, Trigger, Integration, Credential and Profile reads do,
  and the start plan reads what an Organization already holds. `workspace transcript` decodes each
  event by its SSE name through the generated `Event::from_sse` before it shows anything, and
  passes over a name the document does not carry. A record whose columns resolve something the
  decoded record does not hold — an Integration's diagnostic text, which reads the invocation — is
  shown with `typed::show_as`, which prints the columns a terminal or a pipe reads and the answer
  the boundary sent to `--json`. Every other command still declares a `View`
  of dotted paths into an untyped answer, and both draw through one table. Live work inspection
  follows git-style prose even when piped. For commands with a
  `View`, a terminal gets aligned columns and a pipe gets delimited rows. `--json` is a flag on
  every command: it writes the complete operator response as one JSON document — a collection as
  one array, `[]` when empty, and a compound answer such as `workspace open`, `workspace post`,
  `start` or `queue` whole rather than the child a terminal is shown — and a Transcript as one
  record a line, entries and closed Activities as the stream carries them. Nothing is projected or
  clipped; scripts select with `jq`. Any word directly after `--json` (the old projection syntax
  took it as a field list) is refused before anything is sent. `--jq EXPR` (`filter.rs`) filters
  that same complete response through the pinned jaq, and implies JSON: a collection is one array
  input, a followed Transcript is evaluated one record at a time, a string result prints raw and
  any other result as JSON, one a line, and no results is success. The expression is compiled
  before the invocation can reach the control plane, so a malformed filter changes nothing — the
  same hook a future first-run consumer must call. A filter never hides a failure; one that fails
  after a successful write exits 1 and does not resend. The exceptions: `workspace read` writes a binary file's bytes
  raw, a `204` (`forget`, `withdraw`, `acknowledge-refusal`) has no response to write, `status`
  writes the record the Client composes from several reads, and `exit-codes` writes the catalog.
  Raw content has nothing to filter: a `204` has no answer to filter, and `--jq` on `workspace
  read`'s raw bytes is refused rather than inventing a record.
- **Secrets** are read from standard input, never from arguments, with echo off at a terminal.
- **Diagnostics** (`diagnostic/`): every failure reaches stderr as a typed `Diagnostic`, the
  control plane's or one the Client makes for a connection failure, an unreadable or unrecognised
  answer, or a local error. The exit comes from its `kind`, falling back to the HTTP status only for
  kinds [the exit table](operator-boundary.md#errors) does not place. Each `Action` renders as a `kestrel` command that keeps the selected
  `--organization` and, when it is not the default, `--control-plane`; a missing input appears as
  the flag it needs and is never guessed. With `--json` the Diagnostic is one JSON line on stderr
  and stdout holds only successful output. When standard input and standard error are both a
  terminal, the Client offers to run a step: it asks for each missing input, asks for an explicit
  yes before a step with a `consequence`, and keeps the original exit whatever the step does. A
  refused or lost write is never sent again: once an invocation has sent a write, a retry is
  described, not offered to run. A Session's own `diagnostic` is part of
  `session show`'s successful output.
- `kestrel start` (`start.rs`) infers a Project and Agent from the local clone and explains every
  inferred value on stderr before changing anything. Its Organization is the scope's: a
  repository's owner never names one, and only an explicit `--organization` declares one.

## The browser Client

`packages/client` ([README](../../packages/client/README.md)) is a static SPA. The control plane
does not serve it: a web server in front does, on the operator interface's origin
([ADR-0043](../adr/0043-a-web-server-serves-the-browser-client.md)). In compose that is
`images/kestrel-client`, Caddy on the host's loopback at 7719, over plain HTTP with no certificate,
so each tab holds one event stream and subscribes over requests
([ADR-0045](../adr/0045-a-browser-tab-holds-one-stream-and-subscribes-over-requests.md)). It waits
on nothing in compose, so a control plane that cannot start, an incompatible store included, still
leaves a page that says so. Its Caddyfile answers:

| Path | Answer |
| --- | --- |
| `/operator` or under it | Forwarded to the control plane with `Host` and `Origin` unchanged; a `502` `connection_failed` Diagnostic when nothing answered, whose `check_connection` says `compose` when `KESTREL_COMPOSE` is set |
| Under `/assets/` | That file, cached for good, or 404 when absent |
| Anything else | The file if it exists, else `index.html`, uncached, so a deep link survives a refresh |

The Client's types come from
`openapi/operator.json`; its transport (`src/operator/transport.ts`) turns every failure into a
typed diagnostic: the control plane's own, or an `unknown_response`, `connection_failed` or
`client_failure` it makes for a plain `Refusal`, a non-JSON answer, no answer or a fault in the
page, naming its own origin as the control plane. The Caddyfile's `connection_failed` becomes the
same `Unreachable` a failed fetch does, keeping its compose evidence. A read offers reading again after any
`Retry-After`; a write whose answer was lost or unexplained offers inspection, never a resend.
`src/operator/diagnostic-view.ts` binds each next step to a route link that keeps the
Organization, a re-read, a field of the form, a write the person chooses (missing inputs
collected, a destructive one confirmed beside its consequence), or a sentence where the browser
has no screen; an inspection or listing also carries the copyable `kestrel` command that does the
same, as the CLI words it. `src/components/refusal.tsx` renders them, and a failed Session's `diagnostic` in
the Sessions tab. Only an unreachable control plane is an outage
(`src/operator/outage.ts`): `src/components/outage.tsx` covers the still-mounted page with "kestrel
isn't running", the compose commands when the evidence says compose, and checks again at growing
intervals capped at 30 s for 20 attempts, then on request; an answer invalidates every query, so
reads resume without a reload. A refusal or an unreadable answer came from a running control plane
and keeps its own Refusal. `src/operator/tab-stream.ts` holds the tab's one
stream: it reserves lazily, subscribes each live read under its own id, and after any drop reserves
again and re-subscribes from each subscription's current request. A plain `404` on a subscribe is
the reservation forgotten; a typed one is the subscription refused, which waits for the person to
retry it.
`src/operator/follow.ts` holds the two live reads over it: an Organization route's change notices,
whose `open` refetches the Organization and whose changes invalidate TanStack Query keys, and a
Workspace route's Transcript follow, which resumes from its mirror's cursor and renews its
follower's lease. The follow is connecting until its `follower` event says the backlog is replayed,
reconnecting after a drop with what it showed kept, and unavailable when refused or before it was
ever live, so an unreachable stream never reads as an empty Transcript. `src/operator/transcript-view.ts`
is the one mapping from Transcript entries to what a component takes, for the flow and for an opened
Activity alike: a message `said` under a Session's id is the Agent's and is unframed Markdown, anyone
else's is a bubble under their name, and a Session-ended entry says how it went and why it failed.
An Activity (`src/components/workbench/transcript/activity-row.tsx`) is one folded line, open while
the Session works in it, whose entries are read only when it is opened: a slow, empty or failed read
each says which it is, and a failed one keeps the rows it had. A running call is a row from
`session_state` keyed by its `call_id`, which the recorded entry replaces where it stands.
`MessageResponse` (`src/components/ai-elements/message.tsx`) renders the Markdown with Streamdown; no
AI SDK runtime is a dependency. Every queue reader shares
one read (`src/components/workbench/queue-read.tsx`) that says when it is slow, and on failure
names the error, offers a retry and keeps the last known queue. About five tabs fit HTTP/1.1's six connections per origin; nothing detects a
sixth. Browser sign-in progress polls its state read rather than opening a second stream.

A trailing Session's status line (`src/components/workbench/session-status.tsx`) reads its
`observation`, the follow's snapshot before the Session read once it names that Session: a current
one names its open tools and Adapter Units or says it is checking that background work has settled,
and an unavailable one says so and shows any last snapshot as history, never as quiet. The Work tab
keeps `last_report` and `earlier_reports` as history under the Instance that sent each, beside
whatever the live reading says, and says when no report was received. A lease is worded as a
deadline; once the shown deadline passes the Sessions tab re-reads the Organization's Sessions every
15 s, so only the control plane's answer changes a phase or a deadline. A Diagnostic fact that is a
stack, frames with a line and column under what was thrown, opens in AI Elements' Stack Trace
(`src/components/ai-elements/stack-trace.tsx`); any other evidence stays text.

`/setup` names the Operator through the shared producer, then shows saved prerequisites and the
readiness read’s typed gaps, warnings and failed inspections in Questionnaire. The root route resumes
unfinished setup, preserving an explicitly selected Organization when several exist. The durable
install completion read controls onboarding independently of current readiness; a completed install’s
setup route offers contextual repairs. Naming and continuing through saved prerequisites never set
completion. Geist and Geist Mono are bundled once in the root stylesheet for every route.

`/organizations/$organization/settings/agents` lists, declares and edits Agents through the shared
Agent declaration, which is the Agent's whole desired state: an edit sends back every default the
Agent holds, and its name is its identity. The harness catalogue and its image availability decide
which harness may be chosen; one the image is known to lack is withheld, and one whose image could
not be inspected is not. A slow, failed or empty read each says which it is, and a failed read or
refused save keeps what was entered. `src/components/model-choice.tsx` is the one model choice. Given
a live Session's ACP-offered model option it is the AI Elements Model Selector, searching and grouping
exactly what the harness offered, disabled outside a phase that takes option writes and while a
change is pending. Given none, as in Agent settings and the new-Workspace form, it is the harness's
default or a model id taken at its word; it never lists models nobody offered. Provider marks are
bundled in `src/assets/provider-marks` and decorate text that stands without them.
