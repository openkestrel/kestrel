import { describe, expect, test } from "vitest";
import type { Occupant, Queue } from "./generated";
import { openingQueueLine, sessionQueueLine } from "./queue-line";

function an_occupant(name: string): Occupant {
	return {
		name,
		workspace: "019a0000-0000-7000-8000-000000000000",
		agent: "builder",
		phase: "working",
		enqueued_at: "2026-01-01T00:00:00Z",
	};
}

function a_queue(overrides: Partial<Queue> = {}): Queue {
	return {
		work_role: { active_work_slots: 2, serialized_harnesses: ["codex"], driver: "local-exec" },
		active_work: { limit: 2, occupied: 0, occupants: [], elsewhere: 0 },
		instances: { limit: null, count: 0, counted: [] },
		queued: [],
		waiting: [],
		unbriefed: [],
		...overrides,
	};
}

describe("the line the form shows about a Session that has not opened yet", () => {
	test("says order is unknown when no dispatch configuration is recorded", () => {
		expect(openingQueueLine(a_queue({ work_role: null }), true)).toBe(
			"No dispatch configuration is recorded, so queue order is unknown.",
		);
	});

	test("a briefed Session would start when a slot is free", () => {
		expect(openingQueueLine(a_queue(), true)).toBe(
			"The Session would start now: 0 of 2 Active-Work Slots occupied.",
		);
	});

	test("a briefed Session would wait when every slot is occupied", () => {
		const queue = a_queue({
			active_work: {
				limit: 2,
				occupied: 2,
				occupants: [an_occupant("a"), an_occupant("b")],
				elsewhere: 0,
			},
		});
		expect(openingQueueLine(queue, true)).toBe(
			"The Session would wait: all 2 Active-Work Slots are occupied.",
		);
	});

	test("a Briefless Session is not held back by the slots", () => {
		const queue = a_queue({
			active_work: {
				limit: 2,
				occupied: 2,
				occupants: [an_occupant("a"), an_occupant("b")],
				elsewhere: 0,
			},
		});
		expect(openingQueueLine(queue, false)).toBe(
			"Without a Brief, the Session starts preparing at once and waits for its first message.",
		);
	});

	test("never estimates a start time", () => {
		const said = [
			openingQueueLine(a_queue(), true),
			openingQueueLine(
				a_queue({
					active_work: { limit: 1, occupied: 1, occupants: [an_occupant("a")], elsewhere: 0 },
				}),
				true,
			),
		];
		expect(said.join(" ")).not.toMatch(/minute|second|soon|at \d/i);
	});
});

describe("the line a Workspace shows for its queued Session", () => {
	const workspace = "01a0a2d8-baf8-7c02-99fa-7280f174c14a";

	test("names the FIFO position of a ready Session", () => {
		const queue = a_queue({
			queued: [
				{
					position: 2,
					name: "brisk-otter-abcdefgh",
					workspace,
					agent: "builder",
					reasons: [],
					enqueued_at: "2026-09-30T10:00:00Z",
				},
			],
		});
		expect(sessionQueueLine(queue, workspace)).toBe("Queued at position 2.");
	});

	test("names each reason when no position", () => {
		const queue = a_queue({
			queued: [
				{
					position: null,
					name: "brisk-otter-abcdefgh",
					workspace,
					agent: "builder",
					reasons: [{ kind: "live_instance_limit", limit: 1 }],
					enqueued_at: "2026-09-30T10:00:00Z",
				},
			],
		});
		expect(sessionQueueLine(queue, workspace)).toBe("Waiting: at the live Instance limit of 1.");
	});

	test("shows a waiting Turn's global position and foreign predecessors", () => {
		const queue = a_queue({
			waiting: [
				{
					position: 3,
					name: "waiting",
					workspace,
					agent: "builder",
					enqueued_at: "2026-09-30T10:00:00Z",
					pending_since: "2026-09-30T10:01:00Z",
					reasons: [{ kind: "ahead", sessions: ["local"], elsewhere: 1 }],
				},
			],
		});
		expect(sessionQueueLine(queue, workspace)).toBe(
			"Next Turn at position 3. Waiting: behind local and 1 Session in other Organizations.",
		);
	});

	test("shows the first Turn's position", () => {
		const queue = a_queue({
			unbriefed: [
				{
					position: 1,
					name: "unbriefed",
					workspace,
					agent: "builder",
					enqueued_at: "2026-09-30T10:00:00Z",
					preparing: "harness_ready",
					pending_since: null,
					brief_since: "2026-09-30T10:01:00Z",
					reasons: [],
				},
			],
		});
		expect(sessionQueueLine(queue, workspace)).toBe("First Turn at position 1.");
	});

	test("explains unknown order without a recorded dispatch configuration", () => {
		const queue = a_queue({
			work_role: null,
			queued: [
				{
					position: null,
					name: "queued",
					workspace,
					agent: "builder",
					reasons: [],
					enqueued_at: "2026-09-30T10:00:00Z",
				},
			],
		});
		expect(sessionQueueLine(queue, workspace)).toBe(
			"Queue order is unknown: no dispatch configuration is recorded.",
		);
	});

	test("says nothing for a Workspace whose Session is not in the queue", () => {
		expect(sessionQueueLine(a_queue(), workspace)).toBeUndefined();
	});
});
