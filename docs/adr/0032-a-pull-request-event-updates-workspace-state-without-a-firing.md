# A pull request Event updates Workspace state without a Firing

A verified GitHub `pull_request` Event may describe work in an open Workspace, but learning that fact must not itself prompt a Session. After recording the Event, kestrel independently matches its head repository and branch to exactly one open Workspace in the same Organization. It appends an observed pull request change to that Workspace's shared-state Transcript and updates its current pull request state. Trigger evaluation remains separate: a Trigger explicitly declared for the Event may still fire. This refines [ADR-0019](0019-kestrel-declares-the-branch-and-learns-the-pull-request.md) in the vocabulary of [ADR-0030](0030-a-session-is-what-a-harness-user-calls-one.md) and [ADR-0031](0031-a-workspace-fixes-the-place-a-session-chooses-the-agent.md).

## Consequences

- The match uses Organization, head repository and declared branch. A matching branch name in another repository or fork does not attach; zero or multiple candidate Workspaces leave the Event unattached.
- `opened`, `reopened`, `closed` (including a merge), and `synchronize` (head moved) become shared-state entries. Current state holds each observed pull request separately by repository and number, so one branch can have several.
- Without a GitHub Integration for its repository, a Workspace reports pull request status unavailable rather than guessing from git state or Session output.
- A sealed Workspace's Transcript and current state never change, though the Event remains in Organization history.
- At `0.4`, the Audit Record records the unattended attachment verdict, including unmatched, ambiguous and sealed cases.
