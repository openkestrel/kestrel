# Operator boundary and Client

The HTTP API a Client drives the control plane through, and the `kestrel` CLI that is the first
Client ([ADR-0015](../adr/0015-the-cli-is-a-client-not-a-role.md)). Server:
`crates/kestrel/src/operator.rs`. Client: `crates/kestrel-client/`. Contract:
`openapi/operator.json`, which `crates/kestrel/tests/operator.rs` checks against the served routes.

## The boundary

- Served on its own listener (`KESTREL_OPERATOR_LISTEN`, default loopback 7718) and authenticates
  nobody. Operator identity is `0.4` work; until then, reaching the port is authority.
- Every route refuses a `Host` that is not loopback and, when the request carries one, an `Origin`
  that is not exactly `http://` and that `Host`; a cross-origin preflight is granted nothing. The
  guard is `operator::addressed_here`, pinned by `crates/kestrel/tests/forgery.rs`. These checks
  refuse a rebound name and a browser-driven write, and never replace the loopback limit
  ([ADR-0036](../adr/0036-the-browser-client-shares-the-loopback-operator-origin.md), amended by
  [ADR-0043](../adr/0043-a-web-server-serves-the-browser-client.md)).
- Routes live under `/operator/organizations/{organization}/…`, plus a few Organization-free ones
  (`/operator/organizations`, `/operator/starts`, `/operator/events/{record}`). The route table is
  `operator::router`.
- Handlers are thin: parse, call the domain module (`workspace`, `work`, `trigger`, `integration`,
  `start`, `declaration`…), and map the result.

### Errors

Domain code refuses with `declined::Declined` when the caller can act on the reason; the boundary
maps it to a status. Anything else is `Unavailable`.

| Refusal | Status | Client exit |
| --- | --- | --- |
| `Declined::Unacceptable` | 422 | 4 rejected |
| `Declined::Missing`, `Declined::Ambiguous` | 404 | 3 unresolved |
| `Declined::Taken` | 409 | 4 rejected |
| Malformed request, bad cursor | 400 | 4 rejected |
| SQLite busy | 503 with `Retry-After: 1` | 5 unavailable |
| Anything else | 503 | 5 unavailable |

The exit numbers are published by `kestrel exit-codes` and never move (`kestrel-client/src/exit.rs`).
The body of a refusal is the reason alone, written to be shown to a person, plus the `field` the
reason concerns when it is about one (a declared `participant`, for instance).

### References

A Workspace or Session in a path is resolved on the server (`reference.rs`): its generated name, its
UUID, any unambiguous prefix of either, or `latest` for the most recent in scope. Nothing or
several matching is `Declined::Missing` or `Declined::Ambiguous`, naming the candidates.

### Session reads

`GET …/sessions/{session}` serves the Session's whole bookkeeping state — the harness's config
options with their current and offered values, its title for the conversation, and the commands it
offers — kept current by the supervisor for the Session's whole life ([ADR-0041](../adr/0041-a-sessions-options-are-its-harnesss-config-options.md)).
`worked_model` is the Model-category option's current value. The read carries a strong `ETag` over
its body and answers `304` to a matching `If-None-Match`, because a reconnecting Client refetches
every view it subscribes to.

### Streaming the Transcript

`GET …/workspaces/{workspace}/transcript` is SSE. Each entry is an event whose id is a cursor; the
stream ends with an `end` event when the Workspace seals or, with `follow=false`, when it reaches
the last entry. A stream that closes without `end` was cut off; the Client resumes from the last
id it received (`kestrel-client/src/transcript.rs`) and gives up after 30 s unreachable.

`kinds` selects `shared_state`, `narration` or `detail`, defaulting to shared state. The CLI passes
`workspace transcript --kinds` through to this read. Both the link page and this stream advance
across omitted entries: the page returns the highest examined seq, and the stream sends a `cursor`
event when omitted entries advance it beyond the last delivered entry. The cursor remains global.

A follow that stays open past caught-up registers a follower. It is handed one `follower` event
(`id`, `lease_seconds`) and then a `presence` event carrying the whole current set
(`{named, anonymous}`), and another `presence` event whenever the set changes; neither carries an
id, so presence never moves the cursor. `?as=NAME` joins under a name the participant rule
accepts, and names an Agent only to be refused. `POST …/followers/{id}/lease` extends a live
follower's lease and answers `404` for one unknown or lapsed; a dropped stream removes its
follower at once, and a lease that passes removes it and closes its stream. Followers live only in
the serve role's memory ([ADR-0035](../adr/0035-organization-change-notices-and-workspace-presence.md)).
`workspace transcript --follow --as-participant NAME` renews a third of the way through the lease
and prints no presence.

`GET …/workspaces/{workspace}/transcript/payloads/{payload}` fetches an oversized body field as
its original bytes and media type. The entry's `payload_fields` lists fields holding references;
other JSON values remain inline even if they resemble a reference. A reference identifies its
Workspace, entry seq and field. Another Workspace's reference is `404`; a reference whose entry
expired is `410` after its content is removed. Internal prompt and delivery reads resolve references.

### Change notices

`GET …/changes` is SSE, per Organization. It opens with an `open` event, then a `change` event
naming each Workspace, Session or queue that changed, coalesced to at most one per resource per
250 ms at the window's trailing edge. No event carries an id and `Last-Event-ID` is ignored: a
notice is a hint to refetch, never a cursor, and a subscriber that falls behind the bounded
buffer is sent `resync`. The store transaction collects what its writes touched and the hub
publishes after commit, so a refused or rolled-back write raises nothing, and a Transcript append
alone raises nothing. The CLI does not consume it ([ADR-0035](../adr/0035-organization-change-notices-and-workspace-presence.md)).

### Pull requests

A Workspace read carries `pull_requests`: one item per fixed repository, in checkout order.
`availability` is `available` when an inbound, signed GitHub Integration in the Organization
watches the repository, or when one has already delivered a pull request from it (a fork's pull
request arrives through the watched base); `known` then lists its current values. Otherwise it is `unavailable` and `known` is null, which never means there are
none. Nothing is inferred from git or a Session. `kestrel workspace show` prints the same field.

### Reading live work

`GET …/workspaces/{workspace}/work` returns `state: reported`, per-repository summaries and
`reported_at`. It reads Store without taking its write lock and never writes history. With no
Instance it returns `state: no_instance` and the declared branch; its `pull_request` is always null;
learned pull requests are on the Workspace read. A supervisor off the link, or one that has not supplied a summary,
returns `state: not_answering` rather than a durable checkout observation.

`kestrel workspace work <workspace>` (alias `status`) prints a heading per repository and how long
ago its supervisor reported it. `--json` returns the whole operator response.

`GET …/workspaces/{workspace}/files?path=` lists one directory of the live Instance and
`GET …/file?path=&raw=` reads one file, both asked of the supervisor over the link ([Link](link.md#reads)).
`file` answers JSON for inline text and `application/octet-stream` otherwise. A path outside the
checkouts is `422`, a missing one `404`, a Workspace with no Instance `422` naming its branch, and
an Instance that does not answer within 10 s `504`. `kestrel workspace files` (alias `ls`) prints one
entry a line with its git mark; `kestrel workspace read` (alias `cat`) writes the file's bytes to
stdout. `--json` returns the operator response, except for bytes, which are always written raw.

`GET …/workspaces/{workspace}/changes` reads a diff per repository. Its `scope` defaults to
`unpublished`: the working tree and untracked files against `origin/<declared>`, or the merge base
of `HEAD` and `origin/<base>` before the first push. `changed`, `staged` and `commit:<sha>` select
unstaged changes, staged changes and one commit. Repeated `path=<repo>/<path>` parameters filter
literal paths. Patch text across all repositories is capped at 2 MiB; per-file stats remain complete.

`GET …/commits` lists commits reachable from any local branch or detached HEAD that no
remote-tracking branch reaches. `GET …/stashes` lists stashes without diffing them. Both return
text grouped by repository. All three reads use the same transient read path as files.

`kestrel workspace changes` (alias `diff`) accepts `--changed`, `--staged`, a commit id and paths
after `--`. `commits` (alias `log`) and `stashes` (alias `stash`) print git's output under repository
headings. Each command's `--json` returns the operator response.

### Declarations

- `POST …/declaration` applies a whole document (Projects, Agents, Triggers) in one transaction;
  `…/declaration/preview` is the same transaction rolled back and returns the difference.
- `POST …/workspaces` is the one open: it opens a Workspace and enqueues its first Session in one
  transaction, with an optional Brief, declared model, mode, thought level and declared name, or
  refuses and leaves nothing behind ([ADR-0038](../adr/0038-a-session-may-start-before-its-brief.md),
  `workspace.rs`). A refusal names the request `field` it concerns.
- `POST /operator/starts` backs `kestrel start`: it adds whatever is missing to run a first Session
  and refuses to change anything that exists (`start.rs`). It opens through the same write.
- `session enqueue` only continues a Workspace: it refuses one that has never had a Session.

## The Client

- **Where it points**: `KESTREL_CONTROL_PLANE`, default `http://127.0.0.1:7718`.
- **Scope is derived, never stored** ([ADR-0016](../adr/0016-scope-is-derived-never-switched.md),
  `scope.rs`): `--organization`, then `KESTREL_ORGANIZATION`, then a `.kestrel/organization` file
  found walking up from the working directory, then the only Organization that exists. Several
  Organizations and no binding is an error that lists them.
- **Output** (`output.rs`, `view.rs`): most commands declare a `View` of dotted paths into the
  answer. Live work inspection follows git-style prose even when piped, and its `--json` returns
  the whole response. For commands with a `View`, a terminal gets aligned columns; a pipe gets
  delimited rows; `--json a,b` gets exactly those fields in that order.
- **Nothing is prompted for.** Whatever drives the Client may have no terminal. Secrets are read
  from standard input, never from arguments.
- **Corrections** (`corrective.rs`): a refusal the Client recognises is followed by the command
  that would fix it.
- `kestrel start` (`start.rs`) infers an Organization, Project and Agent from the local clone and
  explains every inferred value on stderr before changing anything.

## The browser Client

`packages/client` ([README](../../packages/client/README.md)) is a static SPA. The control plane
does not serve it: a web server in front does, on the operator interface's origin
([ADR-0043](../adr/0043-a-web-server-serves-the-browser-client.md)). In compose that is
`images/kestrel-client`, Caddy on the host's loopback at 7719, whose Caddyfile answers:

| Path | Answer |
| --- | --- |
| `/operator` or under it | Forwarded to the control plane with `Host` and `Origin` unchanged |
| Under `/assets/` | That file, cached for good, or 404 when absent |
| Anything else | The file if it exists, else `index.html`, uncached, so a deep link survives a refresh |

The Client's types come from
`openapi/operator.json`; its transport (`src/operator/transport.ts`) parses a refusal's `message`
and, when present, `field` and `phase`, and reads SSE with `Last-Event-ID` as the cursor.
