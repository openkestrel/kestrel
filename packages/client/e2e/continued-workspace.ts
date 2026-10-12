export type WireEvent = { name: string; id?: string; data: unknown };

export function wire(...events: WireEvent[]): string {
	return events
		.map(
			({ name, id, data }) =>
				`${id ? `id: ${id}\n` : ""}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`,
		)
		.join("");
}

export const FIRST_SESSION = "00000000-0000-0000-0000-0000000000a1";
export const SECOND_SESSION = "00000000-0000-0000-0000-0000000000a2";

export const AGENT_ANSWER = `## Backoff for the follow

The retry loop now **doubles** its wait, capped at *30 seconds*. \`reserve()\` is untouched.

\`\`\`ts
export function wait(attempt: number): number {
	return Math.min(30_000, 500 * 2 ** attempt);
}
\`\`\`

| Attempt | Wait |
| --- | --- |
| 1 | 1s |
| 6 | 30s |

- [x] \`follow.test.ts\` covers the cap
- [ ] the outage page still polls on its own schedule`;

export function continuedTranscript(workspace: string): string {
	let seq = 0;
	const recorded = (
		entry: Record<string, unknown>,
		session: string | null = null,
		kind = "shared_state",
	): WireEvent => {
		seq += 1;
		return {
			name: "entry",
			id: `${workspace}:${seq}`,
			data: {
				kind,
				session_id: session,
				seq,
				appended_at: `2026-09-30T10:${String(seq).padStart(2, "0")}:00Z`,
				entry,
			},
		};
	};
	const agent = (session: string, message: string): WireEvent =>
		recorded(
			{
				type: "said",
				participant: "builder",
				message,
				session_id: session,
				completion: {
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:00:05Z",
					turn_outcome: null,
				},
			},
			session,
		);

	return wire(
		recorded({ type: "participant_joined", participant: "jack" }),
		recorded({
			type: "brief",
			source: { kind: "operator", participant: "jack" },
			brief: "Make the Transcript follow **back off** between retries. See `follow.ts`.",
		}),
		recorded({ type: "session_started", session: FIRST_SESSION, agent: "builder" }, FIRST_SESSION),
		agent(FIRST_SESSION, "I'll start with `tab-stream.ts`, where the reservation is retried."),
		recorded({ type: "participant_joined", participant: "priya" }),
		recorded({
			type: "said",
			participant: "priya",
			message: "Keep the cap *under* a minute — the outage page already waits 30s.",
		}),
		recorded(
			{
				type: "session_ended",
				session: FIRST_SESSION,
				exit: { status: "failed", because: "the harness exited with status 1" },
			},
			FIRST_SESSION,
		),
		recorded({ type: "expired", expired_at: "2026-10-30T10:00:00Z" }, null, "narration"),
		recorded({
			type: "messages",
			messages: [
				{ participant: "jack", message: "Try again, and keep `reserve()` as it is." },
				{ participant: "priya", message: "And add a test for the cap." },
			],
		}),
		recorded(
			{ type: "session_started", session: SECOND_SESSION, agent: "builder" },
			SECOND_SESSION,
		),
		agent(SECOND_SESSION, AGENT_ANSWER),
		recorded({
			type: "pull_request",
			event: "00000000-0000-0000-0000-0000000000e1",
			repository: "https://github.com/openkestrel/kestrel",
			number: 705,
			url: "https://github.com/openkestrel/kestrel/pull/705",
			title: "Back off between Transcript follow retries",
			action: "opened",
			state: "open",
		}),
		recorded(
			{ type: "session_ended", session: SECOND_SESSION, exit: { status: "succeeded" } },
			SECOND_SESSION,
		),
		{ name: "end", data: { because: "sealed" } },
	);
}
