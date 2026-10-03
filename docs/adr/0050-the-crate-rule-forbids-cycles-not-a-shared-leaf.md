# The crate rule forbids cycles between the four crates, not a shared leaf

`docs/architecture/README.md` said "No crate depends on another." It was written as a description
of the four crates that exist — `kestrel`, `kestrel-supervisor`, `kestrel-client`,
`kestrel-scripted-agent` — and of why each side of an HTTP contract defines its own types. Read
literally it also forbids a crate holding nothing but generated wire types, and an architecture
review took it that way when it proposed one home for the operator vocabulary.

The rule's intent is that the four crates never depend on each other and never form a cycle. The
control plane and the Client are separate deployables with separate lifecycles, the supervisor is
dialed out to rather than linked, and no side of an HTTP contract reads the other's types. A leaf
crate that depends on nothing in the workspace and carries only generated types weakens none of
that: the control plane and the Client can both depend on it without either reaching the other.

**A leaf crate that carries generated wire types may be shared**, named
`crates/kestrel-operator-types`, generated from `openapi/operator.json` and depending on no other
kestrel crate. The rule the README states becomes "the four kestrel crates never depend on each
other or form a cycle".

## Considered options

- **Generate each side's types separately.** Two generator runs over one document, two copies of
  the same types, and a divergence possible between them — for no gain, because the types are the
  document.
- **Put the types in one of the four crates and depend across.** This is the dependency the rule
  exists to forbid: the Client's build would sit on the control plane's.

## Consequences

- `crates/kestrel` and `crates/kestrel-client` depend on `kestrel-operator-types`; nothing depends
  on them.
- The browser Client is unchanged: it still generates its own types from the document, and the
  link's two sides still define their own.
- `docs/architecture/README.md` states the rule as its intent rather than as a flat ban.
