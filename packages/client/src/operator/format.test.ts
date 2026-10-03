import { describe, expect, it } from "vitest";
import { ago, preparingStep, reasonText, size } from "./format";

describe("a moment", () => {
	it("reads as how long ago it was", () => {
		const now = Date.parse("2026-09-30T10:00:00Z");
		expect(ago("2026-09-30T09:59:55Z", now)).toBe("just now");
		expect(ago("2026-09-30T09:59:00Z", now)).toBe("a minute ago");
		expect(ago("2026-09-30T09:58:00Z", now)).toBe("2 minutes ago");
		expect(ago("2026-09-30T07:00:00Z", now)).toBe("3 hours ago");
		expect(ago("2026-09-27T10:00:00Z", now)).toBe("3 days ago");
		expect(ago("not a time", now)).toBe("at an unknown time");
	});
});

describe("a size", () => {
	it("reads in bytes, KiB or MiB", () => {
		expect(size(512)).toBe("512 B");
		expect(size(2048)).toBe("2 KiB");
		expect(size(3 * 1024 * 1024)).toBe("3.0 MiB");
	});
});

describe("the preparing step", () => {
	it("names each step from provisioning to harness ready", () => {
		expect(preparingStep("provisioning")).toBe("provisioning the Instance");
		expect(preparingStep("cloning")).toBe("cloning the checkout");
		expect(preparingStep("starting_harness")).toBe("starting the harness");
		expect(preparingStep("harness_ready")).toBe("the harness is ready");
		expect(preparingStep(null)).toBe("preparing");
	});
});

describe("queue reasons", () => {
	it("phrase each structured reason", () => {
		expect(reasonText({ kind: "dependencies", sessions: ["calm-river-abcdefgh"] })).toBe(
			"waiting on calm-river-abcdefgh",
		);
		expect(reasonText({ kind: "live_instance_limit", limit: 2 })).toBe(
			"at the live Instance limit of 2",
		);
		expect(reasonText({ kind: "instance_archiving", instance: "local-1" })).toBe(
			"archiving local-1 to make room",
		);
		expect(reasonText({ kind: "active_work_slots", limit: 4 })).toBe(
			"every Active-Work Slot is occupied (4)",
		);
		expect(
			reasonText({
				kind: "subscription_profile",
				profile: "work",
				session: "calm-river-abcdefgh",
			}),
		).toBe("calm-river-abcdefgh holds the work profile");
		expect(reasonText({ kind: "ahead", sessions: ["a", "b"] })).toBe("behind a and b");
	});
});
