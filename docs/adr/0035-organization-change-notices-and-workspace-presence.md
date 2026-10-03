# Organization changes are hints; Workspace follows carry presence

> **Amended by [ADR-0044](0044-the-browser-client-is-served-over-https.md).** The Client is served
> over HTTPS, so each tab opens its own notice stream and Transcript follow; the third paragraph's
> sharing across tabs, four-stream cap and polling are gone. The stream opens beside the initial
> reads rather than before them, since its open refetches them.

The browser needs prompt changes to Workspace, Session and queue views, but their current values already belong to operator reads. An Organization-level SSE stream sends change notices with the affected resource type and identifier, never a second copy of current state. A Client opens the stream before its initial reads and refetches affected views on a notice; on every open or reconnect it refetches all subscribed views. The notice stream has no durable cursor. The Workspace Transcript keeps its own durable cursor, so losing a notice cannot lose recorded work.

Presence travels on a Workspace Transcript follow as a current snapshot followed by transient updates. Opening a browser or CLI follow registers that follower, and a clean disconnect removes it immediately. A follower that disappears without closing is removed when its one-minute lease expires; the browser or CLI renews the lease with an operator POST while its follow is open. Presence updates carry no Transcript cursor and never enter the Transcript. A reconnect replaces the presence snapshot, while Transcript entries resume from their cursor. Multiple follows under one declared name display once; unnamed follows contribute to an anonymous count. Presence never gates a turn, question or approval.

The browser shares each Organization notice stream across same-origin tabs. A visible tab follows only the Workspace it is viewing and refetches when it becomes visible again. The Client opens at most four SSE connections per origin, leaving two of the six HTTP/1.1 connections available per host for ordinary GETs and POSTs. Views beyond that cap poll their current state and page the Transcript until a follow slot opens. If the tab holding a shared notice stream closes, its replacement opens a stream and refetches. This keeps the same current-state and Transcript guarantees when several tabs are open, while presence remains best-effort.

## Considered options

Streaming full snapshots would duplicate the operator read model and make Client and CLI state disagree when streams are missed. Persisting change notices with a cursor would add a second durable log even though reconnecting Clients must read current state anyway. A separate presence stream would spend another scarce browser connection for state that belongs to an existing follow.
