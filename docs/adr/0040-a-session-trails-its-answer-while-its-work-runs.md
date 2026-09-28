# A Session trails its answer while its work runs

A Harness can answer a Turn and keep working. Claude backgrounds Bash commands and delivers a subagent's follow-up after the response. Codex keeps sending notifications after the prompt returns. OpenCode keeps running child sessions it reports to nobody who has not asked. ACP permits all of this ("updates are not limited to active prompt turns"), and neither v1 nor the v2 draft's `idle` says when background work has ended. Kestrel read the answer as **waiting**, so it reclaimed Instances with an agent mid-push, trusted a stale checkout report for the Unpublished Work hold, and attributed the late output to the next Turn.

Kestrel observes the work rather than preventing it. A Session whose Turn has answered while work the agent started is still running is **trailing**. *Waiting* now promises that nothing kestrel can observe is running. It cannot promise more than a Harness and its Instance show.

- **The baseline needs only ACP.** An activity update after the answer puts the Session into trailing: a message or thought chunk, a `tool_call` or `tool_call_update`, or a `plan`. Bookkeeping updates (usage, model, commands, mode, config, session info) keep updating Session state but never start trailing or extend it. A tool call still open at the answer is an **open background unit**, not closed unresolved at the answer.
- **An adapter may enrich it by declaration.** Where a Harness offers a capability that reports background work with a start and an end, the supervisor declares it and maps its notifications to units opening and settling: Claude's `asyncTasks`, and OpenCode's child-session updates. Without that declaration, OpenCode sends nothing after the response. A Harness with no such capability gets the baseline and nothing else.
- **Trailing ends** when every open unit has settled and 30 seconds have passed without an activity update. A tool call still open then is closed unresolved, as ADR-0034 closes one at a Turn's end.
- **Trailing holds the Active-Work Slot** the Turn held. The slot is released when the work stops, not when the Turn answers.
- **A message or a feeding Firing** starts a Turn at once. Open units carry over in Session state. A Trigger that continues a waiting Session continues a trailing one.
- **Reclaiming under the cap takes only a waiting Session's Instance.** `seal` is refused while trailing. The checkout report taken at the answer is provisional; the supervisor takes a fresh one when trailing ends, and the hold and `seal` read that report.
- **`stop` ends the Harness**, and with it any background work. Kestrel does not drive an adapter's own task-stop control.
- **A Harness that exits cleanly while trailing** ends its Session as a waiting one would. Open units are closed unresolved, which is the visible anomaly. The Turn's response is reported to the work source at the answer, not when trailing ends.

This amends ADR-0034. Output after the answer completes into ordinary Transcript entries by the same unit rules, in completion order, attributed to the Session and to the Turn it follows, never the next. A late agent message is shared state. The end of trailing is a boundary, like a Turn's end. ADR-0037 is unchanged: an Activity after the answer ends at the next shared-state entry or the next Turn's start.

## Considered options

**Preventing it**, by holding `end_turn` until the Harness's work is done, or by configuring each Harness not to background. Rejected: backgrounding is an agent capability, not a fault. OpenCode's children cannot be seen, let alone held, and the configuration would drift with every adapter release.

**Keeping *waiting* as "the Turn answered"** and having reclamation, the hold and `seal` consult a separate busy fact. Rejected: every reader of *waiting* already means "safe to take the Instance", and a second fact would have to be remembered by all of them.

**Relying on adapter capabilities alone.** Rejected: each new ACP Harness would need wiring before kestrel could stop destroying its work. The baseline makes a new Harness safe without any wiring.

**Watching the Instance's process tree.** Rejected: OpenCode's children run inside its own process, and a Harness's MCP servers and language servers would hold the Session in trailing for its whole life.

**Reusing *working***, redefined as "doing something, in a Turn or not". Rejected: a trailing Session has answered, and it takes a new Turn straight away; a working one does neither.

## Consequences

- A Harness that hides background work and sends no update can still leave a Session waiting while something runs. That is a limit of that Harness.
- The quiet period delays reclamation by 30 seconds after the last activity, even when nothing was backgrounded.
- When ACP gains a standard expression for work that outlives a Turn, it becomes the baseline and the adapter translations retire.
