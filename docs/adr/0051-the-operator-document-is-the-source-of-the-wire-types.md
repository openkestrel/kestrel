# The operator document is the source of the wire types

The operator boundary's Rust type layer was hand-written in `operator.rs` — roughly 1,300 lines of
request and response structs and their `From` impls — mirroring `openapi/operator.json`, while the
CLI read responses as untyped values through dotted-path views and the browser Client generated
its types from the same document. One schema change had eight or more edit sites, and no compiler
checked the copies against each other.

**`openapi/operator.json` is the source of the operator wire types.** A generator reads its
`components/schemas` and writes Rust types into `crates/kestrel-operator-types` (ADR-0050). The
control plane and the CLI depend on that crate; the browser Client keeps generating from the same
document with `@hey-api/openapi-ts`. The HTTP contract is unchanged: each side of the link still
defines its own types.

The generator is `openapi-to-rust`, pinned exactly and run by a `generate` bin the crate owns. The
document is the durable asset and the generator is replaceable: the input is the contract, so a
generator that stops being maintained can be swapped without changing what is agreed.

**The generated types are committed, and CI checks they are current** — regeneration followed by
`git diff --exit-code`. They are committed rather than built because they are the contract's Rust
side: a schema change and a generator upgrade should both be visible in review, and a generator
that changes its output between versions must not change the types invisibly. The generator is
pinned exactly, and its output is written formatted so `cargo fmt --all` is a no-op.

**Hand-written mapping stays hand-written.** The builders that need domain knowledge —
`From<Trigger>`'s schedule decomposition, `WorkspaceRecord::read`'s store lookups,
`QueueRecord::of` — live beside the types in the control plane. The generated crate carries no
behaviour and no kestrel knowledge.

## Considered options

- **Keep the hand-written types and make the conformance test exhaustive.** This checks the mirror
  instead of removing it, and leaves the CLI's untyped field access and the eight edit sites in
  place.
- **Generate at build time and commit nothing.** Drift becomes impossible, but a generator bump
  changes the types with no diff anywhere, and the types exist only after a build.
- **Generate separately in each crate.** Two runs over one document, two copies of the same types,
  and a divergence possible between them.
- **Read the schemas with a Draft-7 JSON Schema generator (`typify`).** `typify` consumes roughly
  Draft-7; its maintainer calls the 2020-12 path that OpenAPI 3.1 uses "sometimes works, and often
  breaks", and the document's undiscriminated unions are the code it is least sure of.
  `progenitor` is 3.0.x-only and emits a client kestrel does not want.

## Consequences

- `operator.rs` loses its wire structs and `From` impls endpoint by endpoint; what remains there is
  routing, handlers, and the mapping that needs the domain.
- The hand-written operation pin in the operator tests stays; the required-field presence check
  becomes redundant and goes.
- A change to `openapi/operator.json` is followed by a regeneration and a committed diff. No
  local check is needed until the crate lands; the drift check is a CI concern.
- Wire types carry `String` timestamps and `uuid::Uuid` identifiers. The wire is strings, the
  domain owns time with `jiff`, and the crate takes no second time library.
- The document names its union variants and carries each SSE variant's event name, so the
  name-to-variant mapping is generated rather than hand-maintained in each Client.
