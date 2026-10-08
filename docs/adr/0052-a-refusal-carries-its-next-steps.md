# A refusal carries its next steps

Refusal sentences currently decide HTTP status and corrective commands through string matching.
That makes presentation a contract and leaves the two Clients to guess the repair independently.
The domain instead returns a typed reason and contextual next steps; each Client renders those
steps for the surface the person is using. The decision is recorded in
[Decide how a refusal names its fix](https://github.com/openkestrel/kestrel/issues/490).

The domain vocabulary lives in `declined.rs`. `openapi/operator.json` owns the wire schemas,
generated for the Clients under ADR-0051; the operator boundary maps domain values to those
schemas. A diagnostic carries `kind`, `message`, optional `field`, typed `context`, and ordered
`next_steps`. Prose is display only. A next step names a typed action and its inputs, rather than
a shell command or browser route. Clients preserve the selected control plane and Organization
and collect missing inputs through a form, a terminal prompt, or required flags.

Every error offers a useful next step: a repair when established, inspection when uncertain, or
a retry when temporary. A destructive action names its consequences and requires an explicit
choice. Suggestions execute nothing automatically; a lost response never justifies retrying a
write because the write may have taken effect. Unknown responses and local Client failures get
typed generic diagnostics. Human diagnostics go to stderr; `--json` puts their structured form
on stderr. Successful output stays on stdout. Evidence is sanitized and carries no secret values.

Readiness means there is a usable path to starting work. An unused broken sign-in is a warning;
work that selects it gets a contextual blocking gap. Readiness reads answer HTTP 200 and reuse
the reason and next-step vocabulary. Applying work without a setup prerequisite answers 409 with a
typed gap. A valid partial start preview answers 200 with resolved values, saved prerequisites and
all missing inputs that can be established, without writes or secrets; malformed input and
unavailable inspections retain typed failures. This inspection/apply distinction was agreed in
[Specify the Operator, readiness and CLI 0.4 build tickets](https://github.com/openkestrel/kestrel/issues/535).
The CLI retains exits 0–5 and adds 78 for blocked readiness; bare `kestrel` and
`kestrel status` return 78 when blocked, 0 when ready. Connection failures remain exit 5.

Session failure evidence crosses the link as generic facts: authentication required, a missing
executable, or an unknown failure. The supervisor remains harness-agnostic (ADR-0007). The control
plane adds the Session's harness, image and sign-in context and marks only the sign-in actually
used as needing attention. Expired and not-covered states require evidence establishing those
diagnoses; arbitrary harness prose is diagnostic text, not authority to change sign-in state.

The control plane records a non-secret identity and revision of the sign-in material actually
handed to a Session. A delayed failure marks current saved material as needing attention only
when that revision is still current; replacing the material does not erase the historical Session
diagnosis. Attribution follows the material supplied at execution, rather than assuming that the
material present at enqueue was used. This refinement was agreed in
[Specify the refusal contract's 0.4 build tickets](https://github.com/openkestrel/kestrel/issues/521).

## Considered options

- **Clients infer fixes from refusal kinds.** This duplicates repair knowledge and can make the
  browser and CLI disagree about the same refusal.
- **The control plane supplies shell commands.** It cannot know a Client's shell, local clone,
  missing inputs or preferred interaction surface.
- **Any authentication failure means expired.** A rejected key, missing access or unknown failure
  cannot establish expiry, and an unrelated broken sign-in cannot make every harness unusable.
