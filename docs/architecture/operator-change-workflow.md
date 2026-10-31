# Changing an Operator endpoint

The [Operator boundary](operator-boundary.md) owns routing, errors and domain-to-wire mapping;
[setup](operator-setup.md), [reads and streams](operator-reads.md), and the [Clients](client.md)
describe the consumers. `openapi/operator.json` is the contract, including operation IDs,
required fields and typed refusals.

## Where the document lives

`openapi/operator.json` is the root and stays under the file-size budget by holding only the
document's frame and an index of its paths. Each path's operations live in
`openapi/operator/paths/<area>.json`, keyed by its segments after `/operator` and the
Organization prefix, joined by `-` without parameter braces (`sessions-session-stop`); each schema
lives under its name in `openapi/operator/schemas/<area>.json`. A `$ref` is relative to the file it
is written in: `#/Session` within a schema file, `sessions.json#/Session` from a sibling, `../schemas/sessions.json#/Session` from a path file, and
`../../operator.json#/components/responses/Refused` for the root's shared response.

Nothing reads the files apart. `kestrel_operator_types::document::bundle` (Rust) and
`packages/client/src/operator/document.ts` (the browser Client) assemble one document: every schema
of every schema file a reference reaches joins `components.schemas` under its name, and each
reference becomes `#/components/schemas/<name>`. A name two files define is refused. Both read the
fixtures in `crates/kestrel-operator-types/tests/document/`, so a change to the rules changes both.
Add a path to the index in the root, and a schema to the area file whose paths use it.

## Schema or response changes

1. Change the document and the handler's mapping together. Changed endpoints and their Rust CLI
   consumers use `kestrel-operator-types`, rather than introducing hand-written wire models
   ([ADR-0051](../adr/0051-the-operator-document-is-the-source-of-the-wire-types.md)).
2. Follow the OpenAPI bullet in [AGENTS.md's checks](../../AGENTS.md#checks) for generation and
   consumer checks. Include the committed generated Rust and SSE output in the diff; the browser
   typecheck alone does not regenerate the Rust crate.
3. Check the affected behavior through its existing boundary test. If a response's meaning or
   presentation changes, check its CLI and browser consumers too: generation checks field shapes,
   but cannot establish that a queue explanation or repair command remains readable.

## Added, removed or renamed routes

Update these alongside the document and `operator::router` in `crates/kestrel/src/operator.rs`:

- The explicit `served` inventory in
  `crates/kestrel/tests/suite/operator.rs::the_published_operator_document_describes_the_boundary_the_control_plane_serves`.
  This independent pin must agree with the published paths and methods.
- The `operator::operation` mapping in `crates/kestrel/src/operator.rs`, which supplies the
  documented operation ID to diagnostics.

Run the two existing focused checks after the coherent change:

```sh
cargo test -p kestrel --test suite operator::the_published_operator_document_describes_the_boundary_the_control_plane_serves
cargo test -p kestrel --lib operator::tests::every_route_is_diagnosed_under_the_operation_it_is_documented_as
```

Keep the full-suite run in PR CI, as specified by [AGENTS.md](../../AGENTS.md#checks).
