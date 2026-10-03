# kestrel carries named credentials, never a runtime's own store

> **Amended after [#369](https://github.com/openkestrel/kestrel/issues/369).** The seed premise below
> did not hold. opencode 2 imports a legacy `auth.json` only when an existing database upgrades past
> the import migration; on a fresh home it builds the schema and records every migration complete
> without running their bodies, so the import never fires and the file gives the harness no provider.
> A Profile-held opencode `auth.json` is therefore not a way to sign in. The decision this record
> makes — carry named credentials, never a runtime's own store — is unaffected.

A **Subscription Profile** carries a person's subscription access as things kestrel can name: a
variable, which reaches the Agent Runtime's environment, or a file beneath the agent's home that the
person names ([ADR-0025](0025-subscription-profiles-are-personal.md)). Variables beginning with
`KESTREL_` are reserved for supervisor configuration and cannot be named by a Profile. It does not
carry an Agent Runtime's own credential database, even when that is where the runtime keeps the
login and refreshes it. opencode 2 moved credentials into SQLite
(`~/.local/share/opencode/opencode.db`) and writes refreshed OAuth tokens only there. It never
imports a Profile's `auth.json` into a fresh home, so that file is not a usable sign-in; the variable
`OPENCODE_API_KEY` is. A runtime may add a named credential surface; kestrel will not manufacture one
by reading its store.

**Why not carry the database.** Two reasons, either sufficient. The store is not credentials:
opencode's database also holds the person's sessions, messages and projects, so carrying it would put
one person's runtime history into organization-held, encrypted storage and into every Run that names
the Profile, which a session's read boundary forbids. And reading it would make kestrel depend on
opencode's schema, the opencode-shaped assumption
[ADR-0007](0007-acp-is-the-agent-runtime-contract.md) exists to keep out.

## Considered options

- **Carry `opencode.db` as a Profile file.** Preserves token refresh; rejected above.
- **Add a Profile entry kind for an opaque store**, so the hand-back is atomic and WAL-aware. More
  honest than calling a database a file, but it rejects nothing: the data-boundary and
  schema-coupling objections remain, and the machinery is more than the decision deserves while the
  only such store is opencode's.
- **Keep `auth.json` named and accept the seed.** Chosen, later invalidated by #369: on a fresh home
  the harness never imports it, so the seed reaches no database.

## Consequences

- An OpenCode Go or Zen subscription is a subscription-issued **key**, so it is unaffected: it is a
  variable (`OPENCODE_API_KEY`), and a key does not rotate.
- An opencode OAuth login supplied as `auth.json` has no usable Profile surface: a fresh home never
  imports it (#369). OpenCode signs in with the variable.
- Profiles gain no awareness of any runtime's paths. The image and `USAGE.md` lead with the
  variable, and the usage doc no longer offers a file for OpenCode.
- Durable rotating-login support, if wanted, is requested of the runtime as a nameable credential —
  not retrofitted here.
