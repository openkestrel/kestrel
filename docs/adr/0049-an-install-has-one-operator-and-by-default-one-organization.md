# An install has one Operator and, by default, one Organization

A Subscription Profile needs an owner and a typed message needs a Participant name, and until now
both came from wherever the Client happened to be: `$(id -un)` on the command line, the literal
`operator` elsewhere, and nothing at all in a browser. `kestrel start` also declared an Organization
per repository owner, so a second repository from another owner quietly made a second Organization
on an install the glossary says typically has one.

**An install records one Operator**, the person who runs it, named once when it is set up from either
the CLI or the browser. The Operator is a record with a stable identity, and the name is its label:
a Subscription Profile the Operator signs in to records that identity as its owner, so renaming the
Operator changes nothing ADR-0025 says never changes. The Operator is the default Participant behind
anything typed into a Client. Being named authenticates nobody; ADR-0015's loopback boundary is still
the only authority, and authenticated operator identity is the work of the governance rung.

**Setting up declares one Organization**, named after the Operator, without asking. Every Client
uses the sole Organization whenever there is exactly one; inferring one from a repository's owner is
gone, and `--organization` remains for an install that declares more.

**Work the Operator opens uses the Operator's own sign-in.** A Session opened from a Client with no
profile named takes the Operator's Subscription Profile for its harness when one exists, and the
Organization's Provider Credential otherwise; the Client shows which before applying. A Trigger still
names its profile explicitly, because unattended work must not spend a person's plan by default.

## Considered options

- **Keep a per-Client default name.** The CLI and the browser would disagree about who owns a profile.
- **Ask for an Organization name on first run.** One more question, before the person has done
  anything, about a boundary that means nothing until there is a second one.

## Consequences

- A Profile declared with a free-text `--owner` other than the Operator keeps working as before, and
  is never chosen by default.
- When the governance rung adds authenticated operators, the Operator becomes the first of them.
