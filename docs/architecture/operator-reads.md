# Operator reads and streams

What a Client reads from open work and how it follows it: Session reads, the Transcript stream,
change notices, the per-tab stream and live work. What every route shares is in
[Operator boundary](operator-boundary.md).

## Session reads

`GET …/sessions/{session}` serves the Session's whole bookkeeping state — the harness's config
options with their current and offered values, its title for the conversation, and the commands it
offers — kept current by the supervisor for the Session's whole life ([ADR-0041](../adr/0041-a-sessions-options-are-its-harnesss-config-options.md)).
`worked_model` is the Model-category option's current value. `usage` is what the harness has spent:
the live figure for the Turn in flight, and otherwise what the last Turn's answer or the Session's
end recorded. The read carries a strong `ETag` over its body and answers `304` to a matching
`If-None-Match`, because a reconnecting Client refetches every view it subscribes to.

## Interrupting a Turn

`POST …/sessions/{session}/interrupt` takes `{participant}` and answers `202` with the Session,
whose `interrupting {participant, requested_at}` stands while the request is in flight. Only a
working Turn is interruptible: every other phase is refused `409`, naming it, and a second request
while one is pending answers `202` and sends nothing more. The Turn is cancelled over the link
([Link](link.md#instructions)) without ending the Session, and `kestrel session interrupt <session>
--as-participant NAME` asks for one.

## Streaming the Transcript

`GET …/workspaces/{workspace}/transcript` is SSE. Each entry is an event whose id is a cursor; the
stream ends with an `end` event when the Workspace seals or, with `follow=false`, when it reaches
the last entry. A stream that closes without `end` was cut off; the Client resumes from the last
id it received (`kestrel-client/src/transcript.rs`) and gives up after 30 s unreachable.

`kinds` selects `shared_state`, `narration` or `detail`, defaulting to shared state, and
`summaries` defaults to true. An `activity` event summarizes omitted narration and detail between
every pair of shared-state entries, including when shared state is filtered out. Its `first_seq`
stays stable across pages and reconnects; open updates replace it, and the final `closed` replacement
precedes the closing entry even when no detail was added. Counts and metadata are computed from the
current Transcript at read time, so expired entries contribute only tombstones.

`first_seq` and `last_seq` bound an inclusive expansion range; selecting every kind returns the
entries behind the Activity. Both the link page and this stream advance across omitted entries:
the page returns the highest examined seq and an `activities` array, and entry, Activity and `cursor`
events carry global cursors. The stream sends a `cursor` event when omitted entries advance it
beyond the last delivered entry or Activity.

A follow starts every connect with a transient `session_state` snapshot, including empty state,
then sends changes to its observation, running tools and adapter units, buffering flags and the usage the harness reports — at most
one usage change a second, at the window's trailing edge, and never a row. These events have no id
and are never stored. The CLI passes `--kinds` and `--no-summaries` to the read, prints each closed
Activity once across reconnects, and prints a caught-up open summary on a non-follow read.

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

## Observing open work

A Session read and the `session_state` event carry `observation`. `availability: current` with
`observed_at` says the supervisor's snapshot arrived over its open link and it is still reaching
the control plane; `tools` and `units` then list that snapshot, and empty lists mean nothing is
open. `availability: unavailable` says nothing current is known: before any snapshot, after link
loss or silence, after the Session ends and after a restart. `tools` and `units` are then empty,
and `last` holds the last snapshot this process took, with its `observed_at`, as history only.
A connected or heartbeating supervisor alone, a reconnect, the Session's state and its lease
never make an observation current; only a fresh snapshot does, and availability changes
neither the Session's state nor its scheduling.

## Held messages

A post into a Workspace whose unfinished Session cannot take it leaves a Held Message. The
Workspace read carries `held_messages` — the `held` ones in arrival order, each with `id`,
`participant`, `message`, `posted_at` and `edited_at` — and the post answers
`{ session, held_message }`. `PUT …/workspaces/{workspace}/messages/{id}` takes
`{ participant, message }` and answers the Held Message; `DELETE …/workspaces/{workspace}/messages/{id}`
takes `{ participant }` and answers `204`. Both apply the participant name rule and refuse an id the
Workspace never held `404`, a name other than the author's `403`, and one a Turn took or its author
withdrew `409`. Neither writes a Transcript entry. `kestrel workspace show` lists Held Messages, `workspace post` hands back the Session it reached
(or the held id when it reached none; `--json` writes its whole `{ session, held_message }`
answer), and `workspace message edit` / `workspace message withdraw`
change one.

## Change notices

`GET …/changes` is SSE, per Organization. It opens with an `open` event, then a `change` event
naming each Workspace, Session or queue that changed, coalesced to at most one per resource per
250 ms at the window's trailing edge. No event carries an id and `Last-Event-ID` is ignored: a
notice is a hint to refetch, never a cursor, and a subscriber that falls behind the bounded
buffer is sent `resync`. The store transaction collects what its writes touched and the hub
publishes after commit, so a refused or rolled-back write raises nothing, and a Transcript append
alone raises nothing. The CLI does not consume it ([ADR-0035](../adr/0035-organization-change-notices-and-workspace-presence.md)).

## One stream per tab

A browser tab follows several resources over one SSE connection
([ADR-0045](../adr/0045-a-browser-tab-holds-one-stream-and-subscribes-over-requests.md),
`stream.rs`). `PUT /operator/streams` answers `201` with a `token`; `GET /operator/streams/{token}`
opens it once (a second open is `409`); `PUT …/subscriptions/{id}` takes `{ kind, organization,
workspace?, after?, kinds?, participant? }` and starts or replaces the subscription under that id, and `DELETE`
ends it. An unknown, expired or dropped reservation is `404`, which tells the tab to reserve again.

Each subscription is the per-resource stream it names, built by the same generator: a `notices`
subscription is the Organization's change notices, and a `transcript` one is a follow from `after`
with summaries on, refused as that read refuses; `participant` is that read's `as`. Every event
keeps its per-resource name and carries `{ subscription, cursor?, data }`: `data` is the
per-resource payload and `cursor` the subscription's own Transcript cursor on entry, Activity and
cursor events. No event carries an SSE id. A Transcript subscription registers a follower once
caught up, anonymous unless it names a `participant`, and is renewed through the follower lease
route; replacing, ending or dropping it removes the follower.

The reservation is memory only, bounded at 256 reservations and 32 subscriptions each. One never
opened is forgotten after the follower lease period, and one is forgotten the moment its
connection closes. A subscription that sends `end` is dropped from the connection; one cut off any
other way, such as by a lapsed lease, closes the whole connection, so the tab resumes everything
from its cursors.

## Pull requests

A Workspace read carries `pull_requests`: one item per fixed repository, in checkout order.
`availability` is `available` when an inbound GitHub Integration in the Organization watches
the repository, or when one has already delivered a pull request from it (a fork's pull
request arrives through the watched base); `known` then lists its current values. Otherwise it is `unavailable` and `known` is null, which never means there are
none. Nothing is inferred from git or a Session. `kestrel workspace show` prints the same field.

## Reading live work

`GET …/workspaces/{workspace}/work` returns `state: reported`, per-repository summaries and
`reported_at`. It reads Store without taking its write lock and never writes history. With no
Instance it returns `state: no_instance` and the declared branch; its `pull_request` is always null;
learned pull requests are on the Workspace read. A supervisor off the link, or one that has not supplied a summary,
returns `state: not_answering` rather than a durable checkout observation.

Every state also carries `last_report`: `report: none` until any of the Workspace's Instances has
sent a work report, then the newest complete report with the `instance` that sent it and whether
the Workspace still holds that Instance (`current_instance`). It survives link loss, release and
restart, and a released or replaced Instance's report is never presented as its successor's.
`earlier_reports` holds each other Instance's last report, newest first, so a replaced Instance's
report stays readable after its successor reports. All of it is history only: Unpublished Work, seal, release and reclamation read the numbered `checkout` report,
and Files and Changes still ask the live Instance.

`kestrel workspace work <workspace>` (alias `status`) prints a heading per repository and how long
ago its supervisor reported it, or, when nothing live answers, the last report and its Instance,
then any earlier Instances' reports.
`--json` returns the whole operator response.

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
