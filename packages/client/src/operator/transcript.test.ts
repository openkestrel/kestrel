import { describe, expect, it, vi } from "vitest";
import { cursorSeq, FollowSession, readRange, TranscriptMirror } from "./transcript";
import { diagnosisOf, transport, type StreamEvent } from "./transport";

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
		const session = new FollowSession({
			operations: operator,
			organization: "acme",
			workspace: "brave-otter",
			participant: "operator",
			mirror,
		});

		session.start();
		await vi.waitFor(() => {
			expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1, 2, 3]);
		});
		session.stop();

		expect(transcripts).toBe(2);
		expect(new Headers(seen[0]?.headers).get("last-event-id")).toBeNull();
		expect(new Headers(seen[1]?.headers).get("last-event-id")).toBe("w:2");
	});

	it("stops at a refusal instead of retrying it", async () => {
		let requests = 0;
		const operator = transport(async () => {
			requests += 1;
			return json(404, { message: "no Workspace is named brave-otter" });
		});
		const session = new FollowSession({
			operations: operator,
			organization: "acme",
			workspace: "brave-otter",
			participant: "operator",
			mirror: new TranscriptMirror(),
		});

		session.start();
		await new Promise((resolve) => setTimeout(resolve, 400));

		expect(requests).toBe(1);
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

const follower = eventOf({
	event: "follower",
	id: undefined,
	data: '{"id":"6b1a1f2c-0000-0000-0000-000000000000","lease_seconds":60}',
});

function following(operator: ReturnType<typeof transport>, mirror = new TranscriptMirror()) {
	const session = new FollowSession({
		operations: operator,
		organization: "acme",
		workspace: "brave-otter",
		participant: "operator",
		mirror,
	});
	const states: string[] = [];
	mirror.subscribe(() => {
		const state = mirror.snapshot().connection.state;
		if (states.at(-1) !== state) states.push(state);
	});
	return { session, mirror, states };
}

// A follow the server keeps open, as it does once caught up.
function held(...chunks: string[]) {
	return new ReadableStream<Uint8Array>({
		start(controller) {
			for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
		},
	});
}

describe("a follow's connection", () => {
	it("is connecting until the backlog is replayed, and live once the follower registers", () => {
		const mirror = new TranscriptMirror();

		expect(mirror.snapshot().connection).toEqual({ state: "connecting" });

		mirror.apply({ event: "session_state", id: undefined, data: '{"tools":[]}' });
		mirror.apply(delivered(1));

		expect(mirror.snapshot().connection).toEqual({ state: "connecting" });

		mirror.apply({ event: "follower", id: undefined, data: '{"id":"x","lease_seconds":60}' });

		expect(mirror.snapshot().connection).toEqual({ state: "live" });
	});

	it("reads unavailable, not empty, when the stream cannot be reached at all", async () => {
		const { session, mirror } = following(
			transport(async () => {
				throw new TypeError("Failed to fetch");
			}, "http://localhost"),
		);

		session.start();
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("unavailable"));
		session.stop();

		const connection = mirror.snapshot().connection;
		expect(connection.state === "unavailable" && diagnosisOf(connection.failure).kind).toBe(
			"connection_failed",
		);
		expect(connection.state === "unavailable" && connection.retriesItself).toBe(true);
		expect(mirror.snapshot().entries).toEqual([]);
	});

	it("keeps its entries while reconnecting, and resumes without duplicates", async () => {
		let transcripts = 0;
		let cut: ReadableStreamDefaultController<Uint8Array> | undefined;
		const { session, mirror, states } = following(
			transport(async () => {
				transcripts += 1;
				if (transcripts === 1) {
					return new Response(
						new ReadableStream<Uint8Array>({
							start(controller) {
								cut = controller;
								controller.enqueue(encoder.encode(eventOf(delivered(1)) + follower));
							},
						}),
						{ status: 200, headers: { "content-type": "text/event-stream" } },
					);
				}
				if (transcripts === 2) throw new TypeError("Failed to fetch");
				return new Response(held(eventOf(delivered(1)), eventOf(delivered(2)), follower), {
					status: 200,
					headers: { "content-type": "text/event-stream" },
				});
			}, "http://localhost"),
		);

		session.start();
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("live"));
		cut?.error(new TypeError("network error"));
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("reconnecting"));

		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1]);

		await vi.waitFor(() => expect(transcripts).toBe(3));
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("live"));
		session.stop();

		expect(states).toEqual(["connecting", "live", "reconnecting", "live"]);
		expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1, 2]);
	});

	it("stops at a refusal as unavailable, and follows again when retried", async () => {
		let transcripts = 0;
		const { session, mirror } = following(
			transport(async () => {
				transcripts += 1;
				if (transcripts === 1) return json(404, { message: "no Workspace is named brave-otter" });
				return new Response(held(eventOf(delivered(1)), follower), {
					status: 200,
					headers: { "content-type": "text/event-stream" },
				});
			}),
		);

		session.start();
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("unavailable"));

		const refused = mirror.snapshot().connection;
		expect(refused.state === "unavailable" && refused.retriesItself).toBe(false);
		expect(refused.state === "unavailable" && diagnosisOf(refused.failure).message).toBe(
			"no Workspace is named brave-otter",
		);

		session.retry();
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("live"));
		session.stop();

		expect(transcripts).toBe(2);
	});

	it("reconnects at once when retried during its backoff", async () => {
		let transcripts = 0;
		const { session, mirror } = following(
			transport(async () => {
				transcripts += 1;
				if (transcripts < 4) throw new TypeError("Failed to fetch");
				return new Response(held(eventOf(delivered(1)), follower), {
					status: 200,
					headers: { "content-type": "text/event-stream" },
				});
			}, "http://localhost"),
		);

		session.start();
		await vi.waitFor(() => expect(transcripts).toBe(3), { timeout: 2_000 });
		const before = Date.now();
		session.retry();
		await vi.waitFor(() => expect(mirror.snapshot().connection.state).toBe("live"));
		session.stop();

		expect(Date.now() - before).toBeLessThan(500);
	});
});
