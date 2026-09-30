# Operator boundary and Client

The HTTP API a Client drives the control plane through, and the `kestrel` CLI that is the first
Client ([ADR-0015](../adr/0015-the-cli-is-a-client-not-a-role.md)). Server:
`crates/kestrel/src/operator.rs`. Client: `crates/kestrel-client/`. Contract:
`openapi/operator.json`, which `crates/kestrel/tests/operator.rs` checks against the served routes.

## The boundary

- Served on its own listener (`KESTREL_OPERATOR_LISTEN`, default loopback 7718) and authenticates
  nobody. Operator identity is `0.4` work; until then, reaching the port is authority.
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
The body of a refusal is the reason alone, written to be shown to a person.

### References

A Workspace or Session in a path is resolved on the server (`reference.rs`): its generated name, its
UUID, any unambiguous prefix of either, or `latest` for the most recent in scope. Nothing or
several matching is `Declined::Missing` or `Declined::Ambiguous`, naming the candidates.

### Streaming the Transcript

`GET …/workspaces/{workspace}/transcript` is SSE. Each entry is an event whose id is a cursor; the
stream ends with an `end` event when the Workspace seals or, with `follow=false`, when it reaches
the last entry. A stream that closes without `end` was cut off; the Client resumes from the last
id it printed (`kestrel-client/src/transcript.rs`) and gives up after 30 s unreachable.

### Reading live work

`GET …/workspaces/{workspace}/work` returns `state: reported`, per-repository summaries and
`reported_at`. It reads Store without taking its write lock and never writes history. With no
Instance it returns `state: no_instance` and the declared branch; `pull_request` remains null until
pull request state is built. A supervisor off the link, or one that has not supplied a summary,
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

### Declarations

- `POST …/declaration` applies a whole document (Projects, Agents, Triggers) in one transaction;
  `…/declaration/preview` is the same transaction rolled back and returns the difference.
- `POST /operator/starts` backs `kestrel start`: it adds whatever is missing to run a first Session
  and refuses to change anything that exists (`start.rs`).

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
