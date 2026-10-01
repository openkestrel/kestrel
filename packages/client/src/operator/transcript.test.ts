import { describe, expect, it, vi } from "vitest";
import { cursorSeq, FollowSession, page, readRange, TranscriptMirror } from "./transcript";
import { transport, type StreamEvent } from "./transport";

const encoder = new TextEncoder();

function recorded(seq: number, message = `entry ${seq}`): string {
	return JSON.stringify({
		seq,
		appended_at: "2026-09-30T00:00:00Z",
		kind: "shared_state",
		session_id: null,
		entry: { type: "said", participant: "jack", message },
	});
}

function summary(first: number, last: number, overrides: Record<string, unknown> = {}): string {
	return JSON.stringify({
		first_seq: first,
		last_seq: last,
		counts: { tool_calls: 0, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
		latest: null,
		started_at: null,
		finished_at: null,
		anomaly: false,
		closed: false,
		...overrides,
	});
}

function delivered(seq: number, cursor = `w:${seq}`): StreamEvent {
	return { event: "entry", id: cursor, data: recorded(seq) };
}

function json(status: number, body: unknown) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json" },
	});
}

function events(...chunks: string[]) {
	return new Response(
		new ReadableStream({
			start(controller) {
				for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
				controller.close();
			},
		}),
		{ status: 200, headers: { "content-type": "text/event-stream" } },
	);
}

function eventOf(event: StreamEvent): string {
	return `${event.id ? `id: ${event.id}\n` : ""}event: ${event.event}\ndata: ${event.data}\n\n`;
}

describe("a Transcript mirror", () => {
	it("advances its cursor on entry and cursor events alone", () => {
		const mirror = new TranscriptMirror();

		mirror.apply({ event: "presence", id: undefined, data: '{"named":[],"anonymous":1}' });
		mirror.apply({
			event: "follower",
			id: "w:9",
			data: '{"id":"00000000-0000-0000-0000-000000000000","lease_seconds":60}',
		});
		mirror.apply(delivered(1));

		expect(mirror.cursor).toBe("w:1");

		mirror.apply({ event: "presence", id: undefined, data: '{"named":["jack"],"anonymous":0}' });
		mirror.apply({ event: "session_state", id: "w:8", data: '{"tools":[]}' });

		expect(mirror.cursor).toBe("w:1");

		mirror.apply({ event: "cursor", id: "w:4", data: "w:4" });

		expect(mirror.cursor).toBe("w:4");
	});

	it("parses the sequence from a cursor, and refuses one it cannot read", () => {
		expect(cursorSeq("w:7")).toBe(7);
		expect(cursorSeq("w:abc")).toBeUndefined();
		expect(cursorSeq("w:99999999999999999999")).toBeUndefined();
	});

	it("deduplicates an entry replayed at a sequence already delivered", () => {
		const mirror = new TranscriptMirror();

		mirror.apply(delivered(2));
		mirror.apply(delivered(2));
		mirror.apply(delivered(1));

		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([2]);
		expect(mirror.cursor).toBe("w:2");
	});

	it("keeps the highest cursor as the dedupe floor after a cursor jumps omitted kinds", () => {
		const mirror = new TranscriptMirror();

		mirror.apply({ event: "cursor", id: "w:5", data: "w:5" });
		mirror.apply(delivered(5));
		mirror.apply(delivered(6));

		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([6]);
	});
});

describe("Activity summaries", () => {
	it("replaces an open summary by its first seq, and never reopens a closed one", () => {
		const mirror = new TranscriptMirror();

		mirror.apply({ event: "activity", id: "w:5", data: summary(2, 5) });
		mirror.apply({ event: "activity", id: "w:7", data: summary(2, 7) });

		expect(mirror.snapshot().activities).toHaveLength(1);
		expect(mirror.snapshot().activities[0]?.last_seq).toBe(7);
		expect(mirror.cursor).toBe("w:7");

		mirror.apply({ event: "activity", id: "w:9", data: summary(2, 9, { closed: true }) });
		mirror.apply({ event: "activity", id: "w:7", data: summary(2, 7) });

		expect(mirror.snapshot().activities[0]?.closed).toBe(true);
		expect(mirror.snapshot().activities[0]?.last_seq).toBe(9);
	});

	it("does not deliver an entry the Activity already examined", () => {
		const mirror = new TranscriptMirror();

		mirror.apply({ event: "activity", id: "w:5", data: summary(2, 5) });
		mirror.apply(delivered(3));
		mirror.apply(delivered(6));

		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([6]);
	});
});

describe("transient Session state", () => {
	it("is kept without moving the cursor", () => {
		const mirror = new TranscriptMirror();

		mirror.apply({
			event: "session_state",
			id: undefined,
			data: JSON.stringify({
				session_id: "00000000-0000-0000-0000-000000000001",
				tools: [
					{
						call_id: "call",
						title: "cargo test",
						status: "in_progress",
						started_at: "2026-09-30T00:00:00Z",
					},
				],
				message_buffering: true,
				thought_buffering: false,
			}),
		});

		expect(mirror.snapshot().sessionState?.message_buffering).toBe(true);
		expect(mirror.snapshot().sessionState?.tools[0]?.title).toBe("cargo test");
		expect(mirror.cursor).toBeUndefined();
	});
});

describe("an Activity expansion", () => {
	it("reads its seq range with every kind and collects the entries", async () => {
		const seen: string[] = [];
		const operator = transport(async (url) => {
			seen.push(url);
			return events(
				eventOf(delivered(2)),
				eventOf(delivered(3)),
				eventOf({ event: "end", id: undefined, data: '{"because":"caught_up"}' }),
			);
		});

		const entries = await readRange(operator, "acme", "brave-otter", { first: 2, last: 3 });

		expect(seen[0]).toContain("first_seq=2");
		expect(seen[0]).toContain("last_seq=3");
		expect(seen[0]).toContain("kinds=shared_state%2Cnarration%2Cdetail");
		expect(seen[0]).toContain("follow=false");
		expect(entries.map((entry) => entry.seq)).toEqual([2, 3]);
	});
});

describe("a follow", () => {
	it("resumes from the last cursor after a cut, with no gap or duplicate", async () => {
		const seen: RequestInit[] = [];
		let transcripts = 0;
		const operator = transport(async (_url, init = {}) => {
			seen.push(init);
			transcripts += 1;
			if (transcripts === 1) return events(eventOf(delivered(1)), eventOf(delivered(2)));
			return events(
				eventOf(delivered(2)),
				eventOf(delivered(3)),
				eventOf({ event: "end", id: undefined, data: '{"because":"caught_up"}' }),
			);
		});
		const mirror = new TranscriptMirror();
		const refusals: unknown[] = [];
		const session = new FollowSession({
			operations: operator,
			organization: "acme",
			workspace: "brave-otter",
			participant: "operator",
			mirror,
			onRefused: (error) => refusals.push(error),
		});

		session.start();
		await vi.waitFor(() => {
			expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1, 2, 3]);
		});
		session.stop();

		expect(transcripts).toBe(2);
		expect(new Headers(seen[0]?.headers).get("last-event-id")).toBeNull();
		expect(new Headers(seen[1]?.headers).get("last-event-id")).toBe("w:2");
		expect(refusals).toEqual([]);
	});

	it("hands back a refusal instead of retrying it", async () => {
		let requests = 0;
		const operator = transport(async () => {
			requests += 1;
			return json(404, { message: "no Workspace is named brave-otter" });
		});
		const refusals: { status?: number }[] = [];
		const session = new FollowSession({
			operations: operator,
			organization: "acme",
			workspace: "brave-otter",
			participant: "operator",
			mirror: new TranscriptMirror(),
			onRefused: (error) => refusals.push(error),
		});

		session.start();
		await vi.waitFor(() => {
			expect(refusals).toHaveLength(1);
		});
		await new Promise((resolve) => setTimeout(resolve, 400));

		expect(requests).toBe(1);
		expect(refusals[0]?.status).toBe(404);
	});

	it("renews its lease, and registers again once the lease has lapsed", async () => {
		let transcripts = 0;
		const leases: string[] = [];
		let opened: ReadableStreamDefaultController<Uint8Array> | undefined;
		const operator = transport(async (url, init = {}) => {
			if (init.method === "POST") {
				leases.push(url);
				if (leases.length === 1) return new Response(null, { status: 204 });
				return json(404, { message: "no such follower, or its lease has passed" });
			}

			transcripts += 1;
			const body = new ReadableStream<Uint8Array>({
				start(controller) {
					opened = controller;
					if (transcripts === 1) {
						controller.enqueue(
							encoder.encode(
								eventOf({
									event: "follower",
									id: undefined,
									data: '{"id":"6b1a1f2c-0000-0000-0000-000000000000","lease_seconds":1}',
								}),
							),
						);
					}
				},
			});
			init.signal?.addEventListener("abort", () => opened?.error(new Error("aborted")));
			return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
		});
		const mirror = new TranscriptMirror();
		const session = new FollowSession({
			operations: operator,
			organization: "acme",
			workspace: "brave-otter",
			participant: "operator",
			mirror,
			onRefused: () => {},
		});

		session.start();
		await vi.waitFor(
			() => {
				expect(leases).toHaveLength(1);
			},
			{ timeout: 2_000 },
		);
		await vi.waitFor(
			() => {
				expect(transcripts).toBe(2);
			},
			{ timeout: 2_000 },
		);
		session.stop();

		expect(leases[0]).toBe(
			"/operator/organizations/acme/workspaces/brave-otter/followers/6b1a1f2c-0000-0000-0000-000000000000/lease",
		);
	});
});

describe("a paged read", () => {
	it("reads without following and stops at caught up", async () => {
		const seen: string[] = [];
		const operator = transport(async (url) => {
			seen.push(url);
			return events(
				eventOf(delivered(1)),
				eventOf({ event: "end", id: undefined, data: '{"because":"caught_up"}' }),
			);
		});
		const mirror = new TranscriptMirror();

		await page(operator, "acme", "brave-otter", mirror);

		expect(seen[0]).toContain("follow=false");
		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1]);
	});

	it("resumes a page from the cursor", async () => {
		const headers: Headers[] = [];
		const operator = transport(async (_url, init = {}) => {
			headers.push(new Headers(init.headers));
			return events(eventOf({ event: "end", id: undefined, data: '{"because":"caught_up"}' }));
		});
		const mirror = new TranscriptMirror();
		mirror.apply(delivered(7));

		await page(operator, "acme", "brave-otter", mirror);
		await page(operator, "acme", "brave-otter", mirror);

		expect(headers[0]?.get("last-event-id")).toBe("w:7");
		expect(headers[1]?.get("last-event-id")).toBe("w:7");
	});
});
