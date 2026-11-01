import { describe, expect, it } from "vitest";
import type { Integration } from "./generated";
import { consequenceOf, lifecycleOf, offered, replaced } from "./integrations";

function integration(patch: Partial<Integration> = {}): Integration {
	return {
		id: "00000000-0000-0000-0000-00000000000a",
		name: "github",
		kind: "github",
		repository: "acme/repo",
		bot_login: "kestrel[bot]",
		carries: ["inbound", "outbound"],
		state: "enabled",
		revision: 3,
		disabled_at: null,
		retired_at: null,
		canceled_posts: [],
		polled_every: "1m",
		webhook_path: null,
		last_event_refusal: null,
		diagnostic: null,
		webhook_secret_rotation: null,
		...patch,
	};
}

const rotation: Integration["webhook_secret_rotation"] = {
	started_at: "2026-11-01T10:00:00Z",
	attempted_at: "2026-11-01T10:00:01Z",
	diagnostic: {
		kind: "unavailable",
		message: "GitHub did not acknowledge the new webhook secret",
		field: null,
		context: {
			service: "github",
			resource: "integration",
			operation: "rotate_integration_webhook_secret",
			retry_after_seconds: null,
			saved: null,
		},
		next_steps: [],
	},
};

describe("lifecycleOf", () => {
	it("says what an enabled Integration carries, in its Organization", () => {
		expect(lifecycleOf(integration(), "acme")).toEqual({
			state: "enabled",
			label: "In use",
			detail: "Carries inbound and outbound for acme/repo in acme, polled every 1m.",
		});
	});

	it("says a paused Integration keeps what it resumes with", () => {
		const paused = lifecycleOf(
			integration({ state: "disabled", disabled_at: "2026-11-01T09:00:00Z" }),
			"acme",
		);
		expect(paused.label).toBe("Paused");
		expect(paused.since).toBe("2026-11-01T09:00:00Z");
		expect(paused.detail).toContain("Paused for acme/repo in acme");
		expect(paused.detail).toContain("kept for when it resumes");
	});

	it("says retirement is local and never claims the GitHub App is gone", () => {
		const retired = lifecycleOf(
			integration({ state: "retired", retired_at: "2026-11-01T11:00:00Z" }),
			"acme",
		);
		expect(retired.label).toBe("Retired");
		expect(retired.since).toBe("2026-11-01T11:00:00Z");
		expect(retired.detail).toContain("Retired for acme/repo in acme");
		expect(retired.detail).toContain("untouched on GitHub");
		expect(retired.detail).not.toMatch(/deleted|uninstalled/);
	});
});

describe("offered", () => {
	it("offers pausing, a key, a rotation and retirement to an enabled GitHub Integration", () => {
		expect(offered(integration())).toEqual(["pause", "key", "rotate", "retire"]);
	});

	it("leaves resuming a paused Integration to its diagnostic's step", () => {
		expect(offered(integration({ state: "disabled" }))).toEqual(["key", "rotate", "retire"]);
	});

	it("still offers a rotation while one is unfinished, for the producer to refuse", () => {
		expect(offered(integration({ webhook_secret_rotation: rotation }))).toEqual([
			"pause",
			"key",
			"rotate",
			"retire",
		]);
	});

	it("offers a retired Integration nothing", () => {
		expect(offered(integration({ state: "retired" }))).toEqual([]);
	});

	it("offers a generic webhook no key or rotation", () => {
		expect(offered(integration({ kind: "webhook" }))).toEqual(["pause", "retire"]);
	});
});

describe("consequenceOf", () => {
	it("names the repository a replacement key must still reach", () => {
		expect(consequenceOf("key", integration())).toContain("reach acme/repo");
	});

	it("says retirement is permanent and leaves GitHub alone", () => {
		const said = consequenceOf("retire", integration());
		expect(said).toContain("permanent");
		expect(said).toContain("Nothing changes on GitHub");
	});
});

describe("replaced", () => {
	it("swaps the changed Integration into the list by id", () => {
		const other = integration({ id: "00000000-0000-0000-0000-00000000000b", name: "other" });
		const changed = integration({ state: "disabled", revision: 4 });
		expect(replaced([integration(), other], changed)).toEqual([changed, other]);
	});

	it("leaves a list never read unread", () => {
		expect(replaced(undefined, integration())).toBeUndefined();
	});
});
