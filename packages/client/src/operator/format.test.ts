import { describe, expect, it } from "vitest";
import { ago, leaseLine, preparingStep, reasonText, size } from "./format";

describe("a moment", () => {
	it("reads as how long ago it was", () => {
		const now = Date.parse("2026-09-30T10:00:00Z");
		expect(ago("2026-09-30T09:59:55Z", now)).toBe("just now");
		expect(ago("2026-09-30T09:59:00Z", now)).toBe("a minute ago");
		expect(ago("2026-09-30T09:58:00Z", now)).toBe("2 minutes ago");
		expect(ago("2026-09-30T09:00:00Z", now)).toBe("an hour ago");
		expect(ago("2026-09-30T07:00:00Z", now)).toBe("3 hours ago");
		expect(ago("2026-09-29T08:00:00Z", now)).toBe("a day ago");
		expect(ago("2026-09-27T10:00:00Z", now)).toBe("3 days ago");
		expect(ago("not a time", now)).toBe("at an unknown time");
	});
});

describe("a lease deadline", () => {
	const now = Date.parse("2026-09-30T10:00:00Z");

	it("says how long is left before a deadline still ahead", () => {
		expect(leaseLine("2026-09-30T10:00:20Z", now)).toBe("Lease expires in less than a minute");
		expect(leaseLine("2026-09-30T10:01:00Z", now)).toBe("Lease expires in a minute");
		expect(leaseLine("2026-09-30T10:04:00Z", now)).toBe("Lease expires in 4 minutes");
		expect(leaseLine("2026-09-30T13:00:00Z", now)).toBe("Lease expires in 3 hours");
	});

	it("says a passed deadline expired, never that it was just now", () => {
		expect(leaseLine("2026-09-30T09:59:50Z", now)).toBe("Lease expired less than a minute ago");
		expect(leaseLine("2026-09-30T09:58:00Z", now)).toBe("Lease expired 2 minutes ago");
		expect(leaseLine("2026-09-30T10:00:00Z", now)).toBe("Lease expired less than a minute ago");
	});

	it("does not guess at a deadline it cannot read", () => {
		expect(leaseLine("not a time", now)).toBe("Lease deadline unknown");
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
		expect(reasonText({ kind: "ahead", sessions: [], elsewhere: 2 })).toBe(
			"behind 2 Sessions in other Organizations",
		);

		expect(reasonText({ kind: "ahead", sessions: ["a", "b"] })).toBe("behind a and b");
	});
});

it("uses the custody diagnostic for a non-Session holder", () => {
	expect(
		reasonText({
			kind: "subscription_profile",
			profile: "work",
			session: null,
			diagnostic: {
				kind: "state_conflict",
				field: null,
				message: "the rotating sign-in in the subscription profile work is in use by probe",
				context: {
					operation: "use_sign_in",
					resource: "subscription_profile",
					reference: "work",
					organization: "acme",
					state: "credential_in_use",
					holding_session: null,
				},
				next_steps: [
					{
						action: "inspect_resource",
						resource: "subscription_profile",
						reference: "work",
						organization: "acme",
					},
				],
			},
		}),
	).toBe("the rotating sign-in in the subscription profile work is in use by probe");
});
