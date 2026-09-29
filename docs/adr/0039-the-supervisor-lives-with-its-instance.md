# The supervisor lives with its Instance

A supervisor starts when its Instance is provisioned or resumed, dials the link with the Instance's
identity, and stays until the Instance is destroyed. A Session is begun over that link rather than by
starting a supervisor. This amends [ADR-0002](0002-two-deployables-the-environment-dials-out.md),
which assumed one supervisor per Session, and is what lets the Client read an Instance's files and
changes live between Sessions, when no harness is running.

## Considered options

**Reach in through `exec`.** The compute contract already carries it, and a one-shot supervisor
invocation would answer any read with no change to the link. Rejected: the control plane serving a
browser is not guaranteed to hold the compute handle, a backend's exec may be slow or metered, and
reaching into an Instance is the direction ADR-0002 refused.

**Snapshot the unpublished work at turn close.** Exact between Sessions, because nothing changes a
checkout while no harness runs. Rejected: it persists unbounded diff bytes as a stand-in for a value
that is live one link away.

**A second, read-only process beside a per-Session supervisor.** Doubles the link identities and
still leaves the Session's supervisor a special case.

## Consequences

- **A provider credential still crosses only at the harness's spawn** and dies with the Session
  ([ADR-0010](0010-a-provider-credential-crosses-the-link-at-the-spawn.md)). An Instance between
  Sessions holds a link credential, never a provider credential.
- **Losing the link past a Session's lease ends that Session's harness, not the supervisor.** The
  supervisor keeps redialing for as long as its Instance lives, so reads resume when the control
  plane does.
- **Reads travel the existing link**: a request with an id down the stream, its answer streamed up as
  a POST. The work summary is pushed when it changes. The control plane caches both in memory only;
  the turn-close report the reaping gate reads stays durable and is never shown as current state.
- **The supervisor reads git with `GIT_OPTIONAL_LOCKS=0`**, or a read mid-turn can take `index.lock`
  from under the agent.
- **Once the idle hint exists, the supervisor closes its link when it is sent** and dials again when
  the Instance is resumed, because an open connection must never keep a backend from suspending an
  Instance. [ADR-0018](0018-an-instance-lives-until-its-session-seals.md) hints only an Instance
  holding no Unpublished Work, so a hinted Instance is never read or woken: everything it holds is on
  a remote, and the Client points there.
