# A browser tab holds one stream and subscribes over requests

> **Supersedes [ADR-0044](0044-the-browser-client-is-served-over-https.md).**

ADR-0044 served the browser Client over HTTPS from a local CA, so the browser would speak HTTP/2 and
the six-connections-per-origin limit of HTTP/1.1 would stop mattering. The cost was a step nobody
running `docker compose up` expects: copying a root certificate out of a container and trusting it
by hand, with a browser warning until they do. A first run should need nothing but `docker compose
up`, and until operator identity arrives the only deployment is loopback, so the Client is served
over plain HTTP and lives inside the limit instead.

**A tab holds one event stream for its whole life.** It reserves a stream and receives a token,
opens one SSE connection carrying that token, and then subscribes and unsubscribes with ordinary
requests, each subscription under an id the tab chooses: an Organization's change notices, a
Workspace's Transcript follow. Every event names the subscription it belongs to and that
subscription's own cursor. Navigating changes subscriptions, never the connection. This is the
single-connection mode of [graphql-sse](https://github.com/enisdenjo/graphql-sse/blob/master/PROTOCOL.md),
without GraphQL.

**The tab owns its subscriptions; the server only remembers them.** The server keeps a reservation's
subscriptions in memory, as soft state. `Last-Event-ID` carries one cursor, and this connection
carries several sequences, so it cannot resume one: on any drop, a control-plane restart included,
the tab reserves again and re-subscribes each subscription from the last cursor it saw. Nothing is
missed and nothing arrives twice, by the same cursor rule the per-resource streams already keep.

**The per-resource streams stay.** The CLI and `curl` keep following one Transcript or one
Organization's notices on a connection of their own; the multiplexed stream is what a browser uses,
built from the same sequences.

## Considered options

- **Keep HTTPS and automate the trust step.** Still a step beyond `docker compose up`, and a trust
  store edit is the kind of thing a person should not be asked to do on a first run.
- **One SharedWorker per browser holding one stream for every tab.** No tab limit at all, but it is
  the cross-tab coordination ADR-0044 deleted, in a new costume.
- **Reopen the tab's stream with its subscriptions in the query on every navigation.** Stateless on
  the server, but every navigation is a reconnect.
- **WebSockets.** Not held to the six-connection limit, but a second live transport beside SSE that
  must keep the same cursor semantics.

## Consequences

- About five Kestrel tabs share an origin's six connections before a request stalls. That is
  accepted: there is no detection and no fallback.
- A subscribe request has to reach the process that holds its stream. `serve` runs in one process,
  so it always does; a `serve` split across processes needs sticky routing, the same constraint as
  the live per-Instance state it already holds.
- Caddy serves plain HTTP and holds no CA. The Origin rule of ADR-0044 is unchanged: the listener
  accepts its own host and port under either scheme, so a deployment that puts TLS in front of it
  needs nothing new.
- A remote deployment behind TLS gets HTTP/2 and never meets the limit, but keeps the same protocol.
