# Coding standards

Read at review, with the diff in hand. Each rule lives in one place:

| Source | Holds |
| --- | --- |
| Comments, below | which comments a diff may add |
| `AGENTS.md`, Compatibility | no shims, no backfills; merged migrations are never edited, a change is a new one |
| `docs/architecture/conventions.md` | transactions, errors, abstraction, tests |
| `GLOSSARY.md` | the terms for names, messages and docs, and each entry's _Avoid_ list |
| `packages/client/AGENTS.md` | a diff under `packages/client` |
| The ADRs cited by the architecture page for the area changed | the constraints on that area |

## Comments

A comment earns its place by telling a reader what they would otherwise get wrong: a trap, a
rejected alternative, a constraint an ADR imposes, a consequence that is invisible at the call
site. It is one sentence and it is true of the code as it stands. A better name replaces a comment
that explains a worse one.

List every comment the diff adds, then hold each to that bar:

    git diff <base>...HEAD -U0 -- '*.rs' '*.ts' '*.tsx' '*.sql' ':!**/generated.rs' \
      | grep -E '^\+\s*(//|-- |/?\*)'

A comment that fails it is deleted, whichever of these it is:

- A restatement of the code, including a doc comment that paraphrases the signature below it.
- Roadmap narration (`Nothing sets this yet: the shared start will`). The issue tracker holds the
  plan; the code holds what is true now.
- A module doc that repeats the module's name.

The comments already in a file are not the bar; many predate this rule. A `///` on a clap argument
is help text a person reads, so it is judged as a message, not as a comment.
