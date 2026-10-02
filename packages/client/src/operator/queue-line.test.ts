import { describe, expect, test } from "vitest";
import type { Queue } from "./generated";
import { openingQueueLine, sessionQueueLine } from "./queue-line";

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
	test("says no work role is dispatching when none recorded", () => {
		expect(openingQueueLine(a_queue({ work_role: null }), true)).toBe(
			"No work role is dispatching, so the Session would wait.",
		);
	});

	test("a briefed Session would start when a slot is free", () => {
		expect(openingQueueLine(a_queue(), true)).toBe(
			"The Session would start now: 0 of 2 Active-Work Slots occupied.",
		);
	});

	test("a briefed Session would wait when every slot is occupied", () => {
		const queue = a_queue({
			active_work: { limit: 2, occupied: 2, occupants: ["a", "b"], elsewhere: 0 },
		});
		expect(openingQueueLine(queue, true)).toBe(
			"The Session would wait: all 2 Active-Work Slots are occupied.",
		);
	});

	test("a Briefless Session is not held back by the slots", () => {
		const queue = a_queue({
			active_work: { limit: 2, occupied: 2, occupants: ["a", "b"], elsewhere: 0 },
		});
		expect(openingQueueLine(queue, false)).toBe(
			"Without a Brief, the Session starts preparing at once and waits for its first message.",
		);
	});

	test("never estimates a start time", () => {
		const said = [
			openingQueueLine(a_queue(), true),
			openingQueueLine(
				a_queue({ active_work: { limit: 1, occupied: 1, occupants: ["a"], elsewhere: 0 } }),
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

	test("says nothing for a Workspace whose Session is not in the queue", () => {
		expect(sessionQueueLine(a_queue(), workspace)).toBeUndefined();
	});
});
