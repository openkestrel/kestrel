# The operator document is fragments, and nothing generated from it is committed

ADR-0051 made one file, `openapi/operator.json`, the source of the operator wire types, and
committed the Rust generated from it. Both were the most-conflicted files in the repository once
several agents worked at once: the document was 9,884 lines that 74 of 265 commits edited in six
weeks, and the generated Rust was 12,676 lines that every schema change rewrote. A conflict in
generated code cannot be merged by hand, and a branch that conflicts with `main` gets no CI run.

**The document is authored as one fragment per area in `openapi/operator/`.** A fragment holds
its area's paths and the schemas only that area reaches; `shared.json` holds the rest. Merging
the fragments by key is the document, and a key defined twice is an error. It is still one
contract and still the source of the wire types.

**Nothing generated from it is committed.** `kestrel-operator-types` generates its types in a
build script; the browser Client already generated its own on every build, and now generates its
SSE name mapping the same way. This supersedes ADR-0051's decision to commit the generated types.

The generator stays pinned exactly. ADR-0051 committed the output so a generator upgrade could
not change the types invisibly; the pin is now that review, and a change that breaks a consumer
fails to compile in the control plane or the CLI.

## Considered options

- **Keep one file and commit the generated types.** What ADR-0051 chose, and the source of the
  conflicts.
- **Commit a bundled document beside the fragments.** Every consumer keeps reading one file, but
  the bundle conflicts exactly as the single file did.
- **Keep the generated Rust committed and regenerate to resolve a conflict.** No build cost, but
  the conflicted branch still gets no CI run until someone regenerates.

## Consequences

- Every build of `kestrel-operator-types` compiles the generator as a build dependency, once per
  target directory.
- A schema change shows in review as its fragment only. The generated Rust is read in `OUT_DIR`,
  unformatted.
- The merge is implemented twice, in `kestrel_operator_types::document` and in
  `packages/client/operator-document.ts`, and so is the SSE mapping.
- An image that builds the control plane copies `openapi/operator` into its build.
