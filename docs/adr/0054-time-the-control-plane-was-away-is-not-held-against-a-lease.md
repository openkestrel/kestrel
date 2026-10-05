# Time the control plane was away is not held against a lease

A host sleep during the 0.3 gate (#505) failed every healthy Session. The control plane judges a
lease by the wall clock. The supervisor's give-up timer is monotonic and stood still through the
suspend, so the first sweep after waking ended Sessions whose supervisors were seconds from
renewing. We decided that a gap in the control plane's own running is not held against a lease
beyond a bounded grace. A Session that still expires says the control plane was away.

Each sweep pass records when it ran in `Store`. A first pass that finds the last one more than 10 s
old has found a gap. The cause can be a suspend, a restart, a stopped container or a stalled
database: the control plane cannot tell them apart and does not try. In the same transaction,
before it looks for expired leases, that pass persists the gap and moves every live Session's
lease to at least 30 s from now. Normal failure detection stays at the 2 min lease, and only a gap
adds the 30 s.

Two link changes make grace safe:

- **A heartbeat names the Session it carries** and renews only that one. Without this, a supervisor
  that gave up during a long restart, or on a host that did not sleep, would keep a harness-less
  Session alive just by heartbeating for its Instance.
- **Only the sweep judges a lease passed.** The link refuses reports for a Session that has *ended*,
  not one whose lease has passed. Otherwise a resent report landing before the first sweep
  noticed the gap would be refused, and the supervisor would let a healthy Session go itself.

## Considered Options

- **Attribution only.** Rejected: every laptop sleep would still fail every Waiting Session.
- **Detecting only suspend**, from a jump between the wall and monotonic clocks. Rejected: a restart
  then gets neither grace nor an honest exit, and it is a second mechanism for the same question.
- **An in-memory "don't sweep until T".** Rejected: due times live in `Store`, and a restart
  during grace would lose it.
- **A grace as long as the lease, or configurable.** Rejected: it delays judging a genuinely lost
  supervisor by 2 min after every sleep, for no gain over 15 heartbeats.
