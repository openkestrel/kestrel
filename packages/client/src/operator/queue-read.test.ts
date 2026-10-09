import { describe, expect, test } from "vitest";
import type { Queue } from "./generated";
import { queueRead, queuedSessions } from "./queue-read";
import { Unreachable } from "./transport";

function a_queue(overrides: Partial<Queue> = {}): Queue {
	return {
		work_role: { active_work_slots: 2, serialized_harnesses: [], driver: "local-exec" },
		active_work: { limit: 2, occupied: 2, occupants: [], elsewhere: 0 },
		instances: { limit: null, count: 0, counted: [] },
		queued: [],
		waiting: [],
		unbriefed: [],
		...overrides,
	};
}

const unreachable = new Unreachable(
	"http://localhost:7070",
	{ operation: "GET /", read: true },
	null,
);

describe("a read of the queue", () => {
	test("is reading until it answers, and says so once it is slow", () => {
		expect(queueRead({ data: undefined, error: null, fetching: true }, false)).toEqual({
			kind: "reading",
			delayed: false,
		});
		expect(queueRead({ data: undefined, error: null, fetching: true }, true)).toEqual({
			kind: "reading",
			delayed: true,
		});
	});

	test("stops reading when it fails, and names the failure", () => {
		const read = queueRead({ data: undefined, error: unreachable, fetching: false }, true);
		expect(read).toEqual({ kind: "failed", error: unreachable, known: undefined, retrying: false });
	});

	test("keeps the summary it last read when a later read fails", () => {
		const known = a_queue();
		const read = queueRead({ data: known, error: unreachable, fetching: false }, false);
		expect(read).toMatchObject({ kind: "failed", known });
	});

	test("says it is retrying while a retry after a failure is under way", () => {
		const read = queueRead({ data: undefined, error: unreachable, fetching: true }, false);
		expect(read).toMatchObject({ kind: "failed", retrying: true });
	});

	test("tells an empty queue from one with Sessions in it", () => {
		expect(queueRead({ data: a_queue(), error: null, fetching: false }, false)).toMatchObject({
			kind: "read",
			empty: true,
		});
		const queue = a_queue({
			queued: [
				{
					position: 1,
					name: "agile-robin",
					workspace: "w",
					agent: "builder",
					reasons: [],
					enqueued_at: "2026-09-30T10:00:00Z",
				},
			],
		});
		expect(queueRead({ data: queue, error: null, fetching: false }, false)).toMatchObject({
			kind: "read",
			empty: false,
		});
	});
});

describe("the queued Sessions", () => {
	test("lists queued requests, then Next Turns, then First Turns, each with its standing", () => {
		const queue = a_queue({
			queued: [
				{
					position: 2,
					name: "jolly-wren",
					workspace: "w2",
					agent: "builder",
					reasons: [{ kind: "ahead", sessions: ["agile-robin"] }],
					enqueued_at: "2026-09-30T10:00:00Z",
				},
			],
			waiting: [
				{
					position: 1,
					name: "merry-summit",
					workspace: "w1",
					agent: "builder",
					reasons: [],
					enqueued_at: "2026-09-30T10:00:00Z",
					pending_since: "2026-09-30T10:01:00Z",
				},
			],
			unbriefed: [
				{
					position: null,
					name: "calm-heron",
					workspace: "w3",
					agent: "builder",
					reasons: [],
					enqueued_at: "2026-09-30T10:00:00Z",
					preparing: "cloning",
					pending_since: null,
					brief_since: null,
				},
			],
		});

		expect(queuedSessions(queue)).toEqual([
			{
				name: "jolly-wren",
				workspace: "w2",
				label: "Queued",
				position: 2,
				reasons: "behind agile-robin",
			},
			{ name: "merry-summit", workspace: "w1", label: "Next Turn", position: 1, reasons: "" },
			{ name: "calm-heron", workspace: "w3", label: "First Turn", position: null, reasons: "" },
		]);
	});
});
