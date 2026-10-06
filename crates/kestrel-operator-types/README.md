# Operator wire types

`openapi/operator.json` is the authored contract. This leaf exposes its generated Rust types
and SSE name mappings; domain builders stay in the control plane and presentation stays in
the Clients. Both Rust consumers depend on the leaf, with their endpoint migrations separate.

Run `cargo run -p kestrel-operator-types --bin generate` from any workspace directory to
regenerate `src/generated.rs`, `src/sse.rs`, and the browser's `sse.gen.ts`. The generator is
pinned to `openapi-to-rust =0.19.0` with HTTP/SSE client generation disabled. Rust output is
formatted by the repository's pinned rustfmt. UUIDs use `uuid::Uuid`; timestamps retain their
wire text as `String`. Generated descriptions and serialization helpers are kept intact.

`cargo run -p kestrel-operator-types --bin generate -- --check` checks without writing.
CI also regenerates and rejects a diff. Change the exact dependency pin and regenerate to
review a generator upgrade's output alongside its lockfile change.

Each streaming union's referenced variant carries `x-sse-event`. The generator emits
`event_name()` and `from_sse(name, data)` for Rust, plus name constants and payload maps for
TypeScript. Rust's decoder returns `None` for an unknown name and `Some(Err(_))` for malformed
known data. Use it instead of untagged JSON deserialization: `open` and `resync` have identical
payloads, and the SSE name is the discriminator. Their schema uses `anyOf` because the payload
alternatives overlap. These helpers handle JSON data, not SSE framing.
