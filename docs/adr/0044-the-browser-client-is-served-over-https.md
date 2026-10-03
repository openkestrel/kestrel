# The browser Client is served over HTTPS, so each tab streams on its own

> **Superseded by [ADR-0045](0045-a-browser-tab-holds-one-stream-and-subscribes-over-requests.md).** The
> Client is served over plain HTTP, and each tab holds one stream it subscribes over, so a first run
> needs no certificate.

> **Supersedes the third paragraph of [ADR-0035](0035-organization-change-notices-and-workspace-presence.md)**
> and amends the Origin rule of [ADR-0036](0036-the-browser-client-shares-the-loopback-operator-origin.md).

ADR-0035 had the browser share each Organization's notice stream across same-origin tabs, cap the
Client at four event streams per origin, and poll whatever the cap left out. The cap came from
HTTP/1.1: a browser opens at most six connections to an origin, shared by every tab, and an event
stream holds one for as long as it is open. Living inside that limit took a tab-coordination
protocol in the Client: tabs announcing themselves over a `BroadcastChannel`, a heartbeat and a
liveness timeout, one tab per Organization elected to hold the notice stream and rebroadcast it, a
global ranking of visible tabs for the four slots, and a polling fallback for every view without
one. It was most of the Client's state, and none of it was about kestrel.

A browser speaks HTTP/2 only over TLS, and HTTP/2 carries every request to an origin as a stream on
one connection, with around a hundred allowed at once. So the web server in front of the control
plane (ADR-0043) serves the Client over HTTPS, and the limit the protocol existed for is gone:

- **Each tab opens its own streams.** An Organization view opens that Organization's notice stream;
  a Workspace view also follows that Workspace's Transcript. Nothing is shared between tabs and
  nothing polls.
- **A tab follows while it shows a Workspace, visible or not.** A hidden tab's follower counts in
  presence; presence stays best-effort and never gates anything.
- **The stream opens beside the initial reads, not before them.** Every open refetches the whole
  Organization, which heals any read that raced it, so a route never waits on a stream.

The compose stack's Caddy issues the certificate from its own local CA for `localhost` and
`127.0.0.1`, and keeps the CA on a volume so it survives a restart. The operator trusts that CA
once, by hand; Caddy is told never to edit a trust store.

## The Origin rule

TLS ends at the web server, which forwards plain HTTP to the operator listener. A browser on the
HTTPS front end therefore sends an `Origin` of `https://` with the listener's own host and port.
The listener accepts an `Origin` naming its own host and port under either `http` or `https`. The
scheme added nothing: only something listening on that loopback host and port can serve a page at
that origin, and that is the front end.

## Considered options

- **One multiplexed stream per tab**, carrying notices and the Transcript follow together, would
  keep HTTP/1.1 at the cost of a new endpoint, and still run out at about four tabs.
- **WebSockets** are not held to the six-connection limit, but they would replace SSE and the
  `Last-Event-ID` cursor the CLI and the browser share at the published interface.
- **Accepting the limit**, one stream per view with no coordination, would stall a fifth tab
  without saying why.

## Consequences

- The operator trusts Caddy's local root certificate once per host (USAGE.md). Until they do, the
  browser warns on the Client's origin.
- The Vite dev server stays on HTTP/1.1, so more than two or three development tabs stall. That
  limit is the dev server's, not the Client's.
- The e2e suite drives the Client over HTTPS through the same Caddyfile, ignoring the untrusted
  certificate, and opens six followed tabs at once.
