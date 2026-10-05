import { describe, expect, test } from "vitest";
import type { Occupant, Queue } from "./generated";
import { instancesLine, slotsLine } from "./queue-limits";

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

describe("the header's Active-Work Slot line", () => {
	test("shows the count against the limit", () => {
		expect(slotsLine(a_queue())).toBe("Slots 0/2");
	});

	test("names each local occupant", () => {
		const queue = a_queue({
			active_work: {
				limit: 2,
				occupied: 2,
				occupants: [an_occupant("noble-falcon-xeszjeod"), an_occupant("merry-fox-mohmpkmk")],
				elsewhere: 0,
			},
		});
		expect(slotsLine(queue)).toBe("Slots 2/2: noble-falcon-xeszjeod, merry-fox-mohmpkmk");
	});

	test("counts foreign occupants without naming them", () => {
		const queue = a_queue({
			active_work: {
				limit: 2,
				occupied: 3,
				occupants: [an_occupant("noble-falcon-xeszjeod")],
				elsewhere: 2,
			},
		});
		expect(slotsLine(queue)).toBe("Slots 3/2: noble-falcon-xeszjeod · 2 elsewhere");
	});

	test("names no limit when no dispatch configuration is recorded", () => {
		const queue = a_queue({
			work_role: null,
			active_work: {
				limit: null,
				occupied: 2,
				occupants: [an_occupant("noble-falcon-xeszjeod")],
				elsewhere: 1,
			},
		});
		expect(slotsLine(queue)).toBe(
			"Slots no dispatch configuration recorded · 2 occupied: noble-falcon-xeszjeod · 1 elsewhere",
		);
	});
});

describe("the header's live Instance line", () => {
	test("shows the count against the limit", () => {
		expect(instancesLine(a_queue())).toBe("Instances 0 (no limit)");
	});

	test("names each counted Instance", () => {
		const queue = a_queue({
			instances: { limit: 2, count: 2, counted: ["local-1", "local-2"] },
		});
		expect(instancesLine(queue)).toBe("Instances 2/2: local-1, local-2");
	});

	test("says no limit only when the Organization has none", () => {
		const queue = a_queue({ instances: { limit: null, count: 1, counted: ["local-1"] } });
		expect(instancesLine(queue)).toBe("Instances 1 (no limit): local-1");
	});

	test("counts an Instance it cannot name", () => {
		const queue = a_queue({ instances: { limit: 1, count: 1, counted: [] } });
		expect(instancesLine(queue)).toBe("Instances 1/1");
	});
});
