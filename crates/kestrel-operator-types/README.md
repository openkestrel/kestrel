# Operator wire types

`openapi/operator/` is the authored contract, one fragment per area. This leaf exposes its
generated Rust types and SSE name mappings; domain builders stay in the control plane and
presentation stays in the Clients. Both Rust consumers depend on the leaf.

`build.rs` merges the fragments (`src/document.rs`) and generates the types into `OUT_DIR` on
every build; nothing generated is committed (ADR-0059). The generator is pinned to
`openapi-to-rust =0.19.0` with HTTP/SSE client generation disabled, and the pin is the review a
change in its output gets. UUIDs use `uuid::Uuid`; timestamps retain their wire text as
`String`. Generated descriptions and serialization helpers are kept intact.

Each streaming union's referenced variant carries `x-sse-event`. The build emits `event_name()`
and `from_sse(name, data)` for Rust; the browser Client derives its name constants and payload
maps from the same extension in `packages/client/operator-sse.ts`. Rust's decoder returns `None`
for an unknown name and `Some(Err(_))` for malformed known data. Use it instead of untagged JSON
deserialization: `open` and `resync` have identical payloads, and the SSE name is the
discriminator. Their schema uses `anyOf` because the payload alternatives overlap. These helpers
handle JSON data, not SSE framing.
