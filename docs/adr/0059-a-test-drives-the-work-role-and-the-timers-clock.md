# A test drives the work role and the timer's clock

> **Amends [ADR-0005](0005-six-ports-at-rung-one-are-named-boundaries.md).** `Timer` gets real
> dispatch over its clock: the manual clock is the second implementation the rule of two waits for.

Between 2026-09-18 and 2026-10-16 about twenty tests flaked, and every merge-queue failure in that
window was a flake or an infrastructure error. The diagnosed ones share a cause. A test boots the
whole control plane, whose work role sweeps leases, fires due timers and claims Sessions on its own
schedule, and it reads time from the wall clock. The test can control neither, so it backdates rows
in `Store`, waits in real time, and polls for an observable state against a fixed deadline. That
fails in three ways: the test races the work role for something it meant to do itself, it asserts a
state the product moves past before the poll sees it, or a real-time window closes under CI load.
About half of those diagnoses were real product races, so the fix is to provoke them on purpose,
not to stop them happening.

We decided that a test may drive both:

- **The work role, a pass at a time.** A stepped fixture boots only `serve`. The test calls for one
  work pass, one round of every sweep and one round of dispatch, and gets control back when it is
  done. A pass is the unit the product runs, so a test cannot produce an interleaving the product
  never does. A test that wants the role live turns it on.
- **The control plane's clock.** Every `Timestamp::now` and `Instant::now` in `crates/kestrel/src`
  reads the clock `Timer` holds: the system clock in production, a manual one in a test. It is a
  concrete enum, not a trait, as [ADR-0022](0022-store-repository-traits-and-enum-dispatch-are-deferred-to-the-postgres-rung.md)
  keeps traits for later. The supervisor keeps real time; a test that needs its timing shortens the
  duration, as it does today.
- **Advancing the clock is time the control plane was present.** A test that advances the clock
  runs a work pass at every sweep interval along the way, so the sweep never finds a gap. Time the
  control plane was away ([ADR-0054](0054-time-the-control-plane-was-away-is-not-held-against-a-lease.md))
  is a separate call. Without this, advancing three minutes to expire one lease would read as a gap,
  extend every lease by the grace, and expire none.
- **Each race the work role can lose stays tested.** A stepped test removes the race only from the
  tests that are about something else; at least one test per sweep and per dispatch step interleaves
  it deliberately with what it races.

## Considered options

- **Fix each flake where it lands.** Rejected: each fix is local, and the next test written the same
  way flakes the same way.
- **Step each loop separately** (sweep leases, then dispatch, then fire timers). Rejected: it lets a
  test order work the product never orders, and a failure there is a false one.
- **A seventh `Clock` port.** Rejected: due times already belong to `Timer`.
- **Paused tokio time.** Rejected: the supervisor is a real process on real time, and a paused
  runtime also stalls the I/O the test drives the control plane through.
- **Advancing the clock and backdating the last sweep.** Rejected: it is the row backdating this
  replaces.

## Consequences

- The stepped fixture is opt-in. Tests that have flaked move first; it becomes the default once most
  of `suite` has moved.
- Tests that wait out a real window, such as a 40 s wait for a 30 s grace, become a clock advance and
  stop setting CI's pace.
- Backdating a row to stand in for time is no longer a reason for a `Database` method.
