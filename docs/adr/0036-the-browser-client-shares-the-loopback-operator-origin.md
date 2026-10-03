# The browser Client shares the loopback operator origin

> **Amended by [ADR-0043](0043-a-web-server-serves-the-browser-client.md).** A web server in front of
> the control plane serves the Client, and writes carry no `X-Kestrel-Operator` header; the shared
> origin and the listener's Host and Origin checks stand.
>
> **Amended by [ADR-0044](0044-the-browser-client-is-served-over-https.md).** An `Origin` naming the
> listener's own host and port is accepted under `http` or `https`, since TLS ends at the web server.

The `0.3` browser Client is a static TanStack Start SPA with TanStack Query, built in `packages/client` and copied into the control-plane image. The control plane serves its assets from a fixed runtime directory on the operator listener; `/operator/*` remains reserved for the operator interface, and other Client routes fall back to the SPA shell. This keeps the Client on the same origin as the operator interface without another runtime process or a Client-side store. The SPA does not use Start server functions or server routes to make operator decisions.

`openapi/operator.json` is the source for generated TypeScript types. A small Client transport owns fetch, refusal handling, SSE cursors and reconnects; it uses relative operator URLs. Generating endpoint methods would obscure the stream and error behavior that the CLI and browser must share at the published interface. The built assets, not a source-tree path or a compile-time path, are what the image serves.

## Amendment to ADR-0015's trust limit

The operator listener still authenticates nobody and is reachable only over loopback or a tunnel until operator identity arrives in `0.4`. A browser makes that unauthenticated listener reachable from pages the operator visits, so the listener must reject requests whose `Host` is not an explicitly allowed loopback host. When a request carries `Origin`, it must exactly match the request's own scheme, host and port; cross-origin preflights are refused and no CORS access is granted. Every state-changing operator request must carry `X-Kestrel-Operator`, including CLI writes. The CLI can omit `Origin` but must send that header on writes. A cross-origin form cannot set the header, and a cross-origin script cannot set it without a preflight the listener refuses.

These checks apply to the operator interface, including routes added later. The image and compose setup keep publishing the listener on the host's loopback; the Host and Origin checks protect that publication from DNS rebinding and browser-driven writes, rather than replacing the loopback trust limit.
