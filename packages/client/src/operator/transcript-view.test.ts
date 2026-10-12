import { describe, expect, it } from "vitest";
import type { Activity, Entry, RunningTool } from "./generated";
import { delivered, type Delivered } from "./transcript";
import {
	activityRead,
	activitySteps,
	activitySummary,
	elapsed,
	entryText,
	exitCode,
	exitLine,
	flow,
	payloadReference,
	planStep,
	spoken,
	thoughtLine,
	toolOutput,
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
	it("separates its counts, outcome and duration in the name a screen reader hears", () => {
		const activity = summary(12, 48, {
			counts: { tool_calls: 4, failed_calls: 1, thoughts: 2, plans: 1, tombstones: 1 },
			started_at: "2026-09-30T10:00:00Z",
			finished_at: "2026-09-30T10:02:14Z",
			anomaly: true,
			closed: true,
		});

		expect(activitySummary(activity, false)).toEqual({
			counts: "4 tool calls, 1 failed, 2 thoughts, 1 plan, 1 expired",
			outcome: "interrupted or unresolved",
			duration: "2m 14s",
			name: "Activity, 4 tool calls, 1 failed, 2 thoughts, 1 plan, 1 expired, interrupted or unresolved, 2m 14s",
		});
	});

	it("says a live Activity is working, with no duration yet", () => {
		const activity = summary(2, 5, {
			counts: { tool_calls: 1, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
			started_at: "2026-09-30T10:00:00Z",
			finished_at: "2026-09-30T10:00:03Z",
		});

		expect(activitySummary(activity, true)).toEqual({
			counts: "1 tool call",
			outcome: undefined,
			duration: undefined,
			name: "Activity, Working, 1 tool call",
		});
	});

	it("is still named when nothing was counted", () => {
		expect(activitySummary(summary(2, 5, { closed: true }), false).name).toBe("Activity");
		expect(activitySummary(undefined, true).name).toBe("Activity, Working");
	});
});

function running(call: string, startedAt = "2026-09-30T10:00:00Z"): RunningTool {
	return {
		call_id: call,
		title: `tool ${call}`,
		tool_kind: "execute",
		status: "in_progress",
		started_at: startedAt,
	};
}

function called(seq: number, call: string): Delivered {
	return entry(seq, {
		type: "tool_call",
		session_id: "00000000-0000-0000-0000-0000000000ff",
		call_id: call,
		title: `tool ${call}`,
		tool_kind: "execute",
		status: "completed",
		input: {},
		result: {},
		closing_reason: null,
		completion: {
			started_at: "2026-09-30T10:00:00Z",
			finished_at: "2026-09-30T10:00:01Z",
			turn_outcome: null,
		},
	});
}

describe("an Activity's steps", () => {
	it("puts running calls after what is recorded, oldest first", () => {
		const steps = activitySteps(
			[called(4, "a")],
			[running("c", "2026-09-30T10:00:09Z"), running("b", "2026-09-30T10:00:05Z")],
		);

		expect(steps.map((step) => [step.key, step.kind])).toEqual([
			["call:a", "entry"],
			["call:b", "running"],
			["call:c", "running"],
		]);
	});

	it("gives a completed call the key its running row had, and no second row", () => {
		const before = activitySteps([], [running("a")]);
		const after = activitySteps([called(4, "a")], [running("a")]);

		expect(before.map((step) => step.key)).toEqual(["call:a"]);
		expect(after.map((step) => [step.key, step.kind])).toEqual([["call:a", "entry"]]);
	});

	it("keys anything that is not a call by where it sits in the Transcript", () => {
		const thought = entry(7, {
			type: "thought",
			session_id: "00000000-0000-0000-0000-0000000000ff",
			text: "hm",
			completion: {
				started_at: "2026-09-30T10:00:00Z",
				finished_at: "2026-09-30T10:00:01Z",
				turn_outcome: null,
			},
		});

		expect(activitySteps([thought], []).map((step) => step.key)).toEqual(["seq:7"]);
	});
});

describe("reading an Activity's entries", () => {
	const known = [called(4, "a")];

	it("is delayed, not empty, while the Operator has not answered", () => {
		const waiting = { data: undefined, error: null, fetching: true };

		expect(activityRead(waiting, [], false)).toEqual({
			kind: "reading",
			delayed: false,
			known: [],
		});
		expect(activityRead(waiting, known, true)).toEqual({ kind: "reading", delayed: true, known });
	});

	it("is empty only once a read answered with nothing", () => {
		expect(activityRead({ data: [], error: null, fetching: false }, [], false)).toEqual({
			kind: "empty",
		});
	});

	it("keeps what was last read when a read fails", () => {
		const error = new Error("refused");

		expect(activityRead({ data: undefined, error, fetching: false }, known, false)).toEqual({
			kind: "failed",
			error,
			known,
			retrying: false,
		});
		expect(activityRead({ data: undefined, error, fetching: true }, known, false)).toMatchObject({
			kind: "failed",
			retrying: true,
		});
	});

	it("is the entries once read", () => {
		expect(activityRead({ data: known, error: null, fetching: false }, [], true)).toEqual({
			kind: "read",
			entries: known,
		});
	});
});

describe("a tool's output", () => {
	it("is the text of its content blocks", () => {
		expect(
			toolOutput({
				content: [
					{ type: "content", content: { type: "text", text: "running 2 tests" } },
					{ type: "content", content: { type: "text", text: "ok" } },
				],
				output: { exit_code: 0 },
			}),
		).toEqual({ format: "text", text: "running 2 tests\nok" });
	});

	it("falls back to what the harness reported raw", () => {
		expect(toolOutput({ content: [], output: "plain" })).toEqual({ format: "text", text: "plain" });
		expect(toolOutput({ content: [], output: { stdout: "out\n", stderr: "err" } })).toEqual({
			format: "text",
			text: "out\nerr",
		});
		expect(toolOutput({ content: [], output: { matches: 3 } })).toEqual({
			format: "json",
			text: '{\n  "matches": 3\n}',
		});
	});

	it("is nothing when the call reported nothing", () => {
		expect(toolOutput({ content: [], output: {} })).toBeUndefined();
		expect(toolOutput({ content: [], output: { exit_code: 0 } })).toBeUndefined();
		expect(toolOutput(null)).toBeUndefined();
	});
});

describe("a thought's line", () => {
	it("is its first line without the Markdown marks", () => {
		expect(thoughtLine("The cap belongs in `wait()`.\n\nIt must **never** exceed it.")).toBe(
			"The cap belongs in wait().",
		);
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

describe("who spoke", () => {
	const session = "00000000-0000-0000-0000-000000000001";

	it("is a person unless a Session said it", () => {
		expect(spoken(entry(4, { type: "said", participant: "jack", message: "hello" }))).toEqual([
			{ key: "4:0", participant: "jack", speaker: "person", body: "hello" },
		]);
		expect(
			spoken(
				entry(5, {
					type: "said",
					participant: "builder",
					message: "**done**",
					session_id: session,
					completion: {
						started_at: "2026-09-30T00:00:00Z",
						finished_at: "2026-09-30T00:00:01Z",
						turn_outcome: null,
					},
				}),
			),
		).toEqual([{ key: "5:0", participant: "builder", speaker: "agent", body: "**done**" }]);
	});

	it("keeps each drained message's Participant and order, however many one posted", () => {
		const reference = {
			payload_id: "00000000-0000-0000-0000-000000000001:7:messages.1.message",
			bytes: 70_000,
			media_type: "text/plain; charset=utf-8" as const,
		};

		expect(
			spoken(
				entry(7, {
					type: "messages",
					messages: [
						{ participant: "jack", message: "first" },
						{ participant: "priya", message: reference },
						{ participant: "jack", message: "third" },
					],
				}),
			),
		).toEqual([
			{ key: "7:0", participant: "jack", speaker: "person", body: "first" },
			{ key: "7:1", participant: "priya", speaker: "person", body: reference },
			{ key: "7:2", participant: "jack", speaker: "person", body: "third" },
		]);
	});

	it("is nobody for an entry that is not speech", () => {
		expect(spoken(entry(1, { type: "participant_joined", participant: "jack" }))).toEqual([]);
		expect(spoken(entry(2, { type: "expired", expired_at: "2026-10-30T00:00:00Z" }))).toEqual([]);
	});
});

describe("a Session's exit", () => {
	it("names success, failure and the reported reason", () => {
		expect(exitLine({ status: "succeeded" })).toBe("Session succeeded");
		expect(exitLine({ status: "failed", because: "the harness exited with status 1" })).toBe(
			"Session failed: the harness exited with status 1",
		);
		expect(exitLine({ status: "failed" })).toBe("Session failed");
	});

	it("is what a Session-ended entry is announced as", () => {
		expect(
			entryText({
				type: "session_ended",
				session: "00000000-0000-0000-0000-000000000001",
				exit: { status: "failed", because: "it broke" },
			}),
		).toBe("Session failed: it broke");
	});
});
