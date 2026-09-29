# A Session may start before its Brief

A person can open a Workspace from a Client without writing a Brief. Its first Session is dispatched straight away: it provisions the Instance, checks out the branch, spawns the harness and opens the ACP conversation, then waits **unbriefed**. The first message anyone posts becomes the Brief and the first Turn. This is how kestrel works like an ADE whose agent runs on a remote Instance. Setup happens while the person types, not after they send.

Opening is one atomic operator write: it opens the Workspace and enqueues its first Session, with or without a Brief, or it refuses and leaves nothing behind. It never declares anything. Nothing opens a Workspace without a Session. A firing always renders its Brief, so only an operator's open can start unbriefed.

An unbriefed Session is its Workspace's Unfinished Session. It holds no Active-Work Slot, because nothing is being worked on yet. It does hold an Instance, and it counts against the live Instance limit like a Waiting one. A message posted before the harness is ready waits and becomes the Brief once it is. An unbriefed Workspace nobody uses seals after a day, like any other idle Workspace with no Unpublished Work.

## Considered options

**Letting the Brief become optional**, so the first thing said is an ordinary message. Rejected: a late joiner's page opens with the Brief, and "what was this Workspace for" should always have an answer. The Brief is still the instruction the first Turn carries. It is just written after the Session starts.

**Enqueueing the Session only when the first message arrives.** Rejected: the person would wait through provisioning and cloning after sending their message, which is the delay the ADE mode exists to hide.

**Calling the phase Waiting, or "ready".** Rejected: Waiting means the last prompt was answered, and "ready" already names a queued Session that can be dispatched.

## Consequences

- An operator can hold Instances with Sessions that never work. The live Instance limit and the idle seal bound this. Nothing else does before `0.4`'s Policy.
- `workspace open` and the browser's New Workspace form share one write, and `--brief` is optional on both. `session enqueue` only starts later Sessions in a Workspace that is already open (ADR-0031).
