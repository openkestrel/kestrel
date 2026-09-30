# A web server serves the browser Client

> **Supersedes the serving half of [ADR-0036](0036-the-browser-client-shares-the-loopback-operator-origin.md).**
> The Client still shares the operator interface's origin; the control plane no longer serves it,
> and the `X-Kestrel-Operator` write header is dropped.

ADR-0036 had the control plane serve the Client's built assets from the operator listener, so the
Client shared the operator interface's origin without another process. The origin was the point.
Serving files from the Rust binary was only the means, and it is the wrong one: content types,
cache headers, a single-page fallback, compression and eventually TLS are what a web server is
for, and every place kestrel runs already has one.

So the control plane serves the operator interface and nothing else. The Client is a directory of
built assets, and whatever fronts it must:

- **Put the assets and `/operator` on one origin.** The Client uses relative operator URLs, and the
  listener grants no CORS access (ADR-0036).
- **Forward `Host` and `Origin` unchanged.** The listener refuses a `Host` that is not loopback and
  an `Origin` that is not its own, which is how it refuses a rebound or cross-site request. A
  proxy that rewrites `Host` to the upstream's name is refused.
- **Answer a Client route with `index.html`, uncached, and a missing asset with 404**, so a deep
  link survives a refresh and a stale asset is never answered with HTML.
- **Stream `text/event-stream` responses unbuffered**, or a transcript arrives late.

The compose stack meets these with `images/kestrel-client`: Caddy and the built assets, published
on the host's loopback at 7719. Caddy forwards `Host` and flushes event streams without being told
to; nginx does neither by default. The e2e suite drives the same Caddyfile. Caddy is the compose
stack's choice, not the contract: another platform meets the list its own way, such as a Kubernetes
ingress routing `/operator` to the control plane and everything else to the assets.

## No write header

ADR-0036 also required `X-Kestrel-Operator` on every write, and it was never enforced. It guards
against the cross-site write the Origin check already refuses: a browser sets `Origin` on every
cross-site `POST`, `PUT` and `DELETE`, and a script cannot forge it. A JSON body or a `PUT` or
`DELETE` already needs a preflight the listener refuses; the body-less `POST`s a plain form can
send, such as enabling a Trigger, are refused by `Origin` alone. The header would add protection
only for a client that omits `Origin`, which no browser does on a cross-site write, and it would
cost every Client, the CLI included, a header to remember. So neither Client sends it and the
listener does not ask for it.

## Consequences

- Compose runs a third image. CI builds `kestrel-client` for the compose job, and Main publishes it
  beside the control plane from the same commit; with no compatibility kept, the two are deployed
  together.
- The `kestrel` CLI still dials the operator listener directly.
- A front end on a real domain is refused by the loopback `Host` check. Reaching the Client from
  anywhere but the host still waits for operator identity, as ADR-0015 requires.
