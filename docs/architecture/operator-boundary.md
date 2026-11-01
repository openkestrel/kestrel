# Operator boundary

The HTTP API a Client drives the control plane through. Server: `crates/kestrel/src/operator.rs`.
Contract: `openapi/operator.json` and the files under `openapi/operator/` it refers to
([where the document lives](operator-change-workflow.md#where-the-document-lives)), which
`crates/kestrel/tests/suite/operator.rs` checks against the served routes. This page holds what every route shares: the listener, errors, references and
declarations. [Setup](operator-setup.md), [sign-ins](operator-sign-ins.md) and
[reads and streams](operator-reads.md) have their own pages, as do the [Clients](client.md).

- Served on its own listener (`KESTREL_OPERATOR_LISTEN`, default loopback 7718) and authenticates
  nobody. Reaching the port is authority; the install's Operator record is a name and stable ID,
  not an authenticated identity.
- Every route refuses a `Host` that is not loopback and, when the request carries one, an `Origin`
  that is not exactly `http://` and that `Host`; a cross-origin preflight is granted nothing. The
  guard is `operator::addressed_here`, pinned by `crates/kestrel/tests/suite/forgery.rs`. These
  checks refuse a rebound name and a browser-driven write, and never replace the loopback limit
  ([ADR-0036](../adr/0036-the-browser-client-shares-the-loopback-operator-origin.md), amended by
  [ADR-0043](../adr/0043-a-web-server-serves-the-browser-client.md)).
- Routes live under `/operator/organizations/{organization}/…`, plus a few Organization-free ones
  (`/operator/organizations`, `/operator/starts`, `/operator/events/{record}`). The route table is
  `operator::router`.
- `GET /` answers `text/plain` for a person who opened the port in a browser: it names the API and
  the browser Client's URL, `KESTREL_CLIENT_URL` (default `http://localhost:7719`, validated as an
  `http`/`https` URL). Compose derives it from the Client's published port, so a readiness or
  status read can hand on the configured URL rather than guessing one from the API's port.
- Handlers are thin: parse, call the domain module (`workspace`, `work`, `trigger`, `integration`,
  `start`, `declaration`…), and map the result.

## Errors

Domain code refuses with the typed `declined::Reason`, which the boundary maps to a wire
`Diagnostic` (ADR-0052): `{ kind, message, field?, context, next_steps }`. `context` is a
kind-specific record (a missing reference's resource and scope, an invalid field's name,
constraint and allowed values, a state conflict's resource, current state and holding Session);
`next_steps` is an ordered list of typed `Action`s, never a shell command or a browser route. A
`Reason` about a request field is wrapped in `declined::Concerning`, which sets `field`;
`declined::Within` also names the record the field is in, and the boundary joins the two. `message`
is display only: nothing classifies it, so rewording one cannot change a status or a step. Tokens
in `context` (`declined::Constraint`, a state such as `session_in_flight`, `sealed`,
`no_instance` or a Session's phase) are short and stable, never the display sentence.

A state conflict's, forbidden action's or expiry's steps are a `declined::Next`: an inspection of
the record to look at first, then any steps a person may choose — enqueueing in a Workspace that
continues, stopping the Session that holds it, releasing an Instance that may hold the only copy
of its work. A stopping or releasing step carries a display `consequence`, a typed `effect`
(`fails_session`, `ends_session`, `discards_unpublished_work`) and `requires_choice: true`; a Client
asks before taking one and never takes it as an automatic repair. Held Message refusals inspect
their Workspace, and changing one another participant wrote is a `forbidden_action` on its
author, not a Policy.

A malformed request and a control plane that could not answer are raised without naming an
operation; `operator::diagnosing` fills it in from the route, as the route's `operationId` in
snake case (`operator::operation`, pinned against the document by a unit test). A 503 on a read
offers `retry_read`; on a write it offers `inspect_operation` with `uncertain: true`, because an
unanswered write may have landed and is never replayed. A busy database carries the same delay in
`Retry-After` and `retry_after_seconds`. A private-key replacement GitHub could not check is the
exception: it is refused before anything is written, so its 503 (`service: github`) offers the
replacement again, after `retry_after_seconds` when GitHub named a wait. A declaration whose
selected App could not be asked is refused the same way and offers `retry_request`.

`declined::Declined` remains for producers no ticket has typed yet (Integration registration,
the GitHub App flow and supervisor-relayed read refusals); they answer
a plain `Refusal` (`message` only) on their status, and the shared `Refused` response's
`DiagnosedRefusal` schema admits both, told apart by `kind`. The loopback guard's 403 and an unknown
follower lease's 404 are also plain. Anything else is `Unavailable`.

| Refusal | Status | Client exit |
| --- | --- | --- |
| `Reason::MissingReference`, `Reason::AmbiguousReference` | 404 | 3 unresolved |
| Malformed request, bad cursor | 400 | 4 rejected |
| `Reason::Forbidden` | 403 | 4 rejected |
| `Reason::StateConflict` | 409 | 4 rejected |
| `Reason::Expired` | 410 | 4 rejected |
| `Reason::InvalidField`, `Reason::CredentialRejected` | 422 | 4 rejected |
| `Declined::Unacceptable` | 422 | 4 rejected |
| `Declined::Missing`, `Declined::Ambiguous` | 404 | 3 unresolved |
| `Declined::Taken` | 409 | 4 rejected |
| SQLite busy | 503 with `Retry-After: 1` | 5 unavailable |
| `Reason::GithubUnavailable` | 503 | 5 unavailable |
| `Reason::InstanceTimeout` | 504 | 5 unavailable |
| Anything else | 503 | 5 unavailable |

A `setup_gap` Diagnostic exits 78 (not ready) whatever its status. The exit numbers are published by
`kestrel exit-codes` and never move (`kestrel-client/src/exit.rs`).
The complete Diagnostic/Action contract — every `kind` and `action`, including the setup, failure
and Client-local variants no producer raises yet — is authored once in the operator document,
under `Diagnostic` and `Action` (`openapi/operator/schemas/refusals.json` and `actions.json`);
link evidence grows its producers under its own ticket without changing this shared shape.

## References

A Workspace or Session in a path is resolved on the server (`reference.rs`): its generated name, its
UUID, any unambiguous prefix of either, or `latest` for the most recent in scope. Nothing matching
is a typed `missing_reference` Diagnostic; several matching is `ambiguous_reference`, naming the
candidates as `{id, name}` rather than formatting them into the message.

## Declarations

- `POST …/declaration` applies a whole document (Projects, Agents, Triggers) in one transaction;
  `…/declaration/preview` is the same transaction rolled back and returns the difference.
- Every Declaration goes through `declaration.rs`, whichever route carried it: the single-record
  routes, the document, the applied-Triggers file (`declaration/triggers.rs`) and `start`. Checking
  (field rules, the Trigger parser, the firing budget, harness admission) yields a checked
  declaration, and only a checked one is written, in the caller's transaction. A broken rule is an
  `invalid_field` with the same constraint and message on every path; only the field path differs,
  by the record it is under: `branch`, `project.branch`, `triggers.<name>.brief`.
  The callers choose two things: whether an existing record that differs is changed or refused
  (`start`), and whether flags or a file declared a Trigger.
- The boundary reads the document and the applied-Triggers file as their generated types and maps
  them to the module's own; both apply results answer one `DeclarationChange` (`kind`, `name`,
  `action` over add / change / unchanged / remove, `differences`), and a route chooses whether it
  lists what it left unchanged. A generated type reads past a field it has no place for, so
  `operator::closed` refuses one in a body whose schema the document closes, and the document's
  trigger is refused a schedule field there: both are a `malformed_request`. A Trigger's `on_miss`
  and `on_open_workspace` are plain strings in a request, so the module, not the decoder, refuses
  a spelling it does not know.
- `POST …/workspaces` is the one open: it opens a Workspace and enqueues its first Session in one
  transaction, with an optional Brief, declared model, mode, thought level and declared name, or
  refuses and leaves nothing behind ([ADR-0038](../adr/0038-a-session-may-start-before-its-brief.md),
  `workspace.rs`). A refusal names the request `field` it concerns.
- `POST /operator/starts` backs `kestrel start`: it adds whatever is missing to run a first Session
  and refuses to change anything that exists (`start.rs`). It opens through the same write.
- `session enqueue` only continues a Workspace: it refuses one that has never had a Session.
- A Project's repositories go through one resolver (`repository.rs`) on each of the three paths
  that declare one. It checks syntax only, expands `owner/repo` and `github.com/owner/repo` to
  HTTPS `.git`, and refuses repositories that would share a checkout directory, as a typed
  `invalid_field`. Projects hold the resolved addresses, and the preview diffs against them. It
  contacts no source, so a checkout failure still arrives as supervisor evidence
  ([ADR-0057](../adr/0057-a-repository-declaration-is-not-a-successful-checkout.md)).
  `POST /operator/repository-resolutions` exposes the resolver to Clients without declaring
  anything. `kestrel start` calls it before planning, so it matches Projects by resolved address.
  `purpose: public_setup` accepts HTTPS addresses only and warns that no App-backed push follows.
- A Project declaration selects an Integration per repository with `integrations:
  [{repository, integration}]`; nothing else selects one, neither a GitHub address nor an Event
  ([ADR-0056](../adr/0056-an-integration-keeps-its-identity-through-maintenance.md)). The
  Integration must be an enabled GitHub one that carries outbound and watches that repository
  (`integration::lending`), and the repository is held as HTTPS whatever transport it was given
  by. An SSH address with no selection is kept and promised no credential. The resolution route
  takes the same `integrations` with an `organization` and shows the result, asking GitHub nothing.
- Declaring is two steps, `repository::declarable` then `Declarable::declare`, on all three
  paths. The first runs outside any transaction: for each selection whose binding, repository
  or branch the Project does not already hold, it mints a token as the selected App and reads
  the repository and the branch. A branch GitHub says is absent is a 422 on the branch field; a
  key GitHub rejects is a 409 `credentials_rejected` offering the key's replacement; an
  installation or repository the App does not reach is a 409 that never says the repository does
  not exist; a rate limit or an outage is a 503 offering `retry_request`, since nothing was
  saved. The second runs inside the saving transaction: every selected Integration must still
  be usable and at the revision read, including unchanged bindings. A selection skipped as
  unchanged also requires the Project to still hold it, so maintenance or a redeclaration that
  landed meanwhile is a 409.
  A preview makes the same check and an apply makes its own. A passed check says the App reached
  the repository and branch then; checkout success is still only the supervisor's to report.
- A Project and a Workspace read carry `integrations: [{repository, integration,
  integration_id}]`. A Workspace copies its Project's when it opens and keeps them through a
  rebinding, a disable and a retirement; nothing substitutes another Integration.
- `GET …/integrations/{integration}/repositories?page&per_page` reads the repositories the
  Integration's installation reaches from GitHub at each call, with each default branch and
  whether it is the one the Integration `watched`. A read GitHub refuses or does not answer is
  the same typed refusal, offering `retry_read`, never an empty page or a guessed branch.
  `kestrel integration repositories NAME --page N --per-page N` reads the same page; `--json`
  keeps its total, current page and next page alongside the repository metadata.
