import { describe, expect, it } from "vitest";
import type { Activity, Entry } from "./generated";
import { delivered, type Delivered } from "./transcript";
import {
	activityCounts,
	activityWindow,
	elapsed,
	entryText,
	exitCode,
	flow,
	payloadReference,
	planStep,
	toolState,
} from "./transcript-view";

function summary(first: number, last: number, overrides: Partial<Activity> = {}): Activity {
	return {
		first_seq: first,
		last_seq: last,
		counts: { tool_calls: 0, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
		latest: null,
		started_at: null,
		finished_at: null,
		anomaly: false,
		closed: false,
		...overrides,
	};
}

function entry(seq: number, value: Entry): Delivered {
	return delivered({
		seq,
		appended_at: "2026-09-30T00:00:00Z",
		kind: "shared_state",
		session_id: null,
		entry: value,
	});
}

describe("the flow", () => {
	it("orders Activities and shared-state entries by the seq they start at", () => {
		const items = flow(
			[
				entry(1, { type: "participant_joined", participant: "jack" }),
				entry(9, { type: "said", participant: "jack", message: "done" }),
			],
			[summary(2, 5), summary(6, 8)],
		);

		expect(
			items.map((item) => (item.kind === "entry" ? item.entry.seq : item.activity.first_seq)),
		).toEqual([1, 2, 6, 9]);
	});
});

describe("an Activity summary line", () => {
	it("names its window and counts", () => {
		const activity = summary(12, 48, {
			counts: { tool_calls: 3, failed_calls: 1, thoughts: 2, plans: 1, tombstones: 1 },
			anomaly: true,
		});

		expect(activityWindow(activity)).toBe("12–48");
		expect(activityCounts(activity)).toBe("3 tools, 1 failed, 2 thoughts, 1 plan, 1 expired");
	});

	it("is empty when nothing was omitted", () => {
		expect(activityCounts(summary(2, 5))).toBe("");
	});
});

describe("elapsed time", () => {
	it("scales from milliseconds to hours", () => {
		expect(elapsed(340)).toBe("340ms");
		expect(elapsed(2_400)).toBe("2.4s");
		expect(elapsed(42_000)).toBe("42s");
		expect(elapsed(125_000)).toBe("2m 5s");
		expect(elapsed(3_725_000)).toBe("1h 2m");
		expect(elapsed(-1)).toBeUndefined();
	});
});

describe("a tool's state", () => {
	it("maps the wire statuses to the words the pane shows", () => {
		expect(toolState("pending")).toBe("running");
		expect(toolState("in_progress")).toBe("running");
		expect(toolState("completed")).toBe("completed");
		expect(toolState("failed")).toBe("failed");
	});

	it("reads how kestrel closed a call over the status the harness last reported", () => {
		expect(toolState("in_progress", "interrupted")).toBe("interrupted");
		expect(toolState("pending", "unresolved")).toBe("unresolved");
		expect(toolState("in_progress", "failed")).toBe("failed");
		expect(toolState("completed", null)).toBe("completed");
	});
});

describe("a tool's exit code", () => {
	it("is read from the result where a harness put one", () => {
		expect(exitCode({ content: [], output: { exit_code: 2 } })).toBe(2);
		expect(exitCode({ output: { exitCode: 0 } })).toBe(0);
		expect(exitCode({ exit_code: 130 })).toBe(130);
		expect(exitCode({ content: [], output: { text: "done" } })).toBeUndefined();
		expect(exitCode("not an object")).toBeUndefined();
	});
});

describe("a payload reference", () => {
	it("is recognised by its fields", () => {
		expect(
			payloadReference({
				payload_id: "00000000-0000-0000-0000-000000000001:7:message",
				bytes: 70_000,
				media_type: "text/plain; charset=utf-8",
			}),
		).toMatchObject({ bytes: 70_000, media_type: "text/plain; charset=utf-8" });
		expect(payloadReference({ message: "inline" })).toBeUndefined();
		expect(payloadReference(null)).toBeUndefined();
	});
});

describe("a plan step", () => {
	it("is read even though the generated plan entry refers to itself", () => {
		expect(planStep({ content: "ship it", priority: "high", status: "in_progress" })).toEqual({
			content: "ship it",
			priority: "high",
			status: "in_progress",
		});
		expect(planStep({ message: "not a step" })).toBeUndefined();
	});
});

describe("entry text", () => {
	it("names what an entry is about", () => {
		expect(entryText({ type: "said", participant: "jack", message: "hello" })).toBe("jack: hello");
		expect(
			entryText({
				type: "tool_call",
				session_id: "00000000-0000-0000-0000-000000000001",
				call_id: "call",
				title: "cargo test",
				tool_kind: "execute",
				status: "completed",
				input: {},
				result: {},
				closing_reason: null,
				completion: {
					started_at: "2026-09-30T00:00:00Z",
					finished_at: "2026-09-30T00:00:01Z",
					turn_outcome: null,
				},
			}),
		).toBe("cargo test");
		expect(entryText({ type: "expired", expired_at: "2026-09-30T00:00:00Z" })).toBe("expired");
	});
});
