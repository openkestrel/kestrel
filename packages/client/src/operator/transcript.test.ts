import { describe, expect, it, vi } from "vitest";
import { json, reservingControlPlane } from "./stream-fake";
import { TabStream } from "./tab-stream";
import { cursorSeq, FollowSession, readRange, TranscriptMirror } from "./transcript";
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

function following(
	participant: string | null,
	answer?: (url: string, init: RequestInit) => Response | undefined,
) {
	const server = reservingControlPlane(answer);
	const stream = new TabStream(server.operations);
	const mirror = new TranscriptMirror();
	const session = new FollowSession({
		stream,
		operations: server.operations,
		organization: "acme",
		workspace: "brave-otter",
		participant,
		mirror,
	});
	return { server, stream, mirror, session };
}

describe("a follow", () => {
	it("subscribes under the participant's name, and resumes from its last cursor after a drop", async () => {
		const { server, stream, mirror, session } = following("operator");

		session.start();
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		const id = server.puts[0]?.id ?? "";
		server.send(id, "entry", JSON.parse(recorded(1)), "w:1");
		server.send(id, "entry", JSON.parse(recorded(2)), "w:2");
		await vi.waitFor(() => expect(mirror.cursor).toBe("w:2"));
		server.drop();
		await vi.waitFor(() => expect(server.opens).toHaveLength(2));
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		server.send(id, "entry", JSON.parse(recorded(2)), "w:2");
		server.send(id, "entry", JSON.parse(recorded(3)), "w:3");

		await vi.waitFor(() => {
			expect(mirror.snapshot().entries.map((entry) => entry.seq)).toEqual([1, 2, 3]);
		});
		session.stop();
		stream.close();

		expect(server.puts.map((put) => put.body)).toEqual([
			{
				kind: "transcript",
				organization: "acme",
				workspace: "brave-otter",
				participant: "operator",
			},
			{
				kind: "transcript",
				organization: "acme",
				workspace: "brave-otter",
				after: "w:2",
				participant: "operator",
			},
		]);
	});

	it("ends its subscription when it stops, keeping the tab's connection", async () => {
		const { server, stream, session } = following(null);

		session.start();
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		session.stop();

		await vi.waitFor(() => expect(server.deletes).toHaveLength(1));
		expect(server.puts[0]?.body.participant).toBeUndefined();
		expect(server.reservations()).toBe(1);
		stream.close();
	});

	it("stops at a refusal instead of retrying it", async () => {
		const { server, stream, mirror, session } = following("operator");
		server.refusing(() =>
			json(404, {
				kind: "missing_reference",
				message: "no Workspace is named brave-otter",
				context: {},
				next_steps: [],
			}),
		);

		session.start();
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		await new Promise((resolve) => setTimeout(resolve, 400));
		stream.close();

		expect(server.puts).toHaveLength(1);
		expect(mirror.snapshot().entries).toEqual([]);
	});

	it("renews its lease, and subscribes again once the lease has lapsed", async () => {
		const leases: string[] = [];
		const { server, stream, mirror, session } = following("operator", (url, init) => {
			if (init.method !== "POST") return undefined;
			leases.push(url);
			if (leases.length === 1) return new Response(null, { status: 204 });
			return json(404, { message: "no such follower, or its lease has passed" });
		});

		session.start();
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		const id = server.puts[0]?.id ?? "";
		server.send(id, "entry", JSON.parse(recorded(1)), "w:1");
		server.send(id, "follower", {
			id: "6b1a1f2c-0000-0000-0000-000000000000",
			lease_seconds: 1,
		});
		await vi.waitFor(() => expect(leases).toHaveLength(2), { timeout: 2_000 });
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		session.stop();
		stream.close();

		expect(leases[0]).toBe(
			"/operator/organizations/acme/workspaces/brave-otter/followers/6b1a1f2c-0000-0000-0000-000000000000/lease",
		);
		expect(server.puts[1]).toMatchObject({ id, body: { after: "w:1" } });
		expect(mirror.cursor).toBe("w:1");
		expect(server.reservations()).toBe(1);
	});
});
