# A suite test is stepped unless it is listed

> **Amends [ADR-0059](0059-a-test-drives-the-work-role-and-the-timers-clock.md).** Stepping stops
> being opt-in until most of `suite` has moved; it is the default now.

ADR-0059 named why `suite` flakes: a test boots a live work role on the wall clock and races it. It
made the stepped fixture opt-in until most of `suite` had moved. Two weeks later about 58 of some
1,038 `suite` tests boot stepped, and every new test copies a live one beside it, so the flakes it
diagnosed keep arriving at the rate they always did while old ones are fixed one by one.

We decided that a test in `suite/` boots stepped unless it is named on an allowlist, and a check
that fails the build refuses any other live boot, as `suite/clock.rs` refuses a stray clock read:

- **The allowlist names tests, not files**, and it only shrinks. A new test is never added to it.
- **A deliberate race test is a permanent entry, with its reason.** ADR-0059 keeps one per sweep and
  dispatch step, and those must stay live.
- **Editing a listed test does not force its migration.** A pull request that touches a live test
  for another reason stays the size of that reason; modules migrate in their own pull requests.

## Considered options

- **Keep it opt-in until most of `suite` has moved.** Rejected: the inflow never stops, so "most"
  never arrives.
- **A rule in `CODING_STANDARDS.md` that review enforces.** Rejected: the live pattern is the one
  every neighbouring test shows, and review has not caught it so far.
- **An allowlist of files.** Rejected: a new test added to a listed file would boot live unchecked.
- **Migrate a listed test whenever it is edited.** Rejected: it makes unrelated changes carry a
  migration and a stress run.
