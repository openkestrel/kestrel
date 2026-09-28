# A Transcript read summarizes the Activity it omits

ADR-0020 made shared state alone what a late joiner reads. That kept 439 tool calls out of their way, but it also hid that any work happened between two messages. A late joiner now reads shared state plus a summary of each **Activity**: the narration and detail between two consecutive shared-state entries. A Workspace holds at most one Unfinished Session, so Activities never interleave. A Session boundary or a Turn's first message always ends one.

A Transcript page or follow names the kinds it returns and whether it summarizes the ones it leaves out. A summary covers only the omitted kinds and gives:

- its first and last seq;
- counts of tool calls, failed calls, thoughts, plans and tombstones;
- the latest item's kind, title and status;
- the earliest start and latest finish;
- whether any call was interrupted or unresolved.

The cursor is always the global seq, and a filtered page's next cursor is the highest seq it examined. Expanding an Activity pages its seq range with its kinds included. A payload over 64 KiB is fetched by reference and is gone once expired. The browser Client and the CLI both default to shared state with summaries.

Summaries are computed when read and never stored. A stored summary would be a second record that could disagree with the Transcript, and it would keep facts past ADR-0033's retention window. After expiry a summary keeps only counts from tombstones. Having the Client page every entry to draw one line is what this read exists to avoid.

A follow re-sends an open Activity's summary as the Activity grows. Each update replaces the previous one by its first seq and advances the cursor. The running tool calls, and whether a thought or message is still buffering, are Session state under ADR-0034. They travel on the Workspace follow as transient updates alongside presence (ADR-0035): a snapshot, then changes, with no cursor, no partial text, and nothing added to the Transcript. The Session read carries its current tool calls for Workspaces nobody is following. An Organization change notice fires when a call starts or settles, never on an intermediate update, because a notice per update would cause a refetch storm in tool-heavy Turns.
