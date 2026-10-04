# A Dependency is declared when its Session is enqueued

An operator declares a Session's Dependencies in the same write that enqueues it: opening a Workspace or enqueueing a Session in one carries `depends_on`, and either every edge is recorded with the Session or the request is refused and nothing is left behind. No request adds a Dependency to a Session that already exists. A Session being enqueued cannot yet be anyone's blocker, so a cycle cannot be formed and there is no cycle rule to enforce. Dispatch also never sees the Session before its edges, so it cannot start the Session before the edges hold it.

A blocker that has already ended successfully is accepted as satisfied. One that has ended without success, or is Unreachable, is refused: no Dependency on it could ever be met, and the cascade that marks dependents Unreachable runs only when a blocker ends, so such a Dependency would leave its Session queued forever. Tolerance stays all-must-succeed. Choosing another tolerance, or becoming Unreachable at birth instead of being refused, belongs to Workflows.

## Considered options

**Declaring a Dependency on a Session that is already queued.** Rejected: it needs cycle detection, and it races dispatch claiming the Session, which could start before the edge lands.

**A Trigger field naming the Sessions a firing waits on.** Rejected: a Trigger is configuration and has no Session to name when it is declared, and its existing blocker is the Work Item's readiness (ADR-0023), a different relation. Waiting on what another firing started is a Workflow's job.
