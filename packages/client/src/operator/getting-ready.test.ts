import { describe, expect, it } from "vitest";
import type { Session, Workspace } from "./generated";
import { continueDraft, preparingStep, sessionStatusLine } from "./getting-ready";

function session(overrides: Partial<Session> = {}): Session {
	return {
		id: "00000000-0000-0000-0000-000000000001",
		name: "calm-river-abcdefgh",
		workspace: "00000000-0000-0000-0000-0000000000ff",
		state: "unbriefed",
		preparing: null,
		exit: null,
		outcome_message: null,
		instance: null,
		supervisor: null,
		agent: "builder",
		harness: "opencode",
		model: null,
		mode: null,
		thought_level: null,
		worked_model: null,
		title: null,
		options: [],
		commands: [],
		enqueued_at: "2026-09-30T10:00:00Z",
		started_at: null,
		ended_at: null,
		lease_expires_at: null,
		connected_at: null,
		supervisor_version: null,
		usage: null,
		changing_options: [],
		interrupting: null,
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

function workspace(overrides: Partial<Workspace> = {}): Workspace {
	return {
		id: "00000000-0000-0000-0000-0000000000ff",
		name: "brave-otter-abcdefgh",
		organization: "acme",
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: { repositories: [], base: "main", branch: "kestrel/0000" },
		instance: null,
		held: null,
		held_messages: [],
		correlation: null,
		state: "sealed",
		opened_at: "2026-09-30T10:00:00Z",
		last_active_at: "2026-09-30T10:00:00Z",
		sealed_at: "2026-09-30T11:00:00Z",
		continues: null,
		started_by: null,
		continued_by: [],
		pull_requests: [],
		unfinished_session: null,
		...overrides,
	};
}

describe("the preparing line", () => {
	it("follows the Session's step from provisioning to harness ready", () => {
		expect(preparingStep("provisioning")).toBe("provisioning the Instance");
		expect(preparingStep("cloning")).toBe("cloning the checkout");
		expect(preparingStep("harness_ready")).toBe("the harness is ready");
		expect(preparingStep(null)).toBe("preparing");

		expect(sessionStatusLine(session({ preparing: "provisioning" }))).toBe(
			"Getting ready: provisioning the Instance…",
		);
		expect(sessionStatusLine(session({ preparing: "cloning" }))).toBe(
			"Getting ready: cloning the checkout…",
		);
		expect(sessionStatusLine(session({ preparing: "harness_ready" }))).toBe(
			"Ready: the harness is up, and your first message becomes the Brief.",
		);
	});

	it("shows a failed or unreachable Session, and nothing once a Session is under way", () => {
		expect(
			sessionStatusLine(
				session({
					state: "ended",
					exit: { status: "failed", because: "the spawn failed" },
				}),
			),
		).toBe("The Session failed: the spawn failed.");
		expect(
			sessionStatusLine(session({ state: "ended", exit: { status: "succeeded" } })),
		).toBeUndefined();
		expect(sessionStatusLine(session({ state: "unreachable" }))).toBe(
			"The Session is unreachable: its supervisor was lost.",
		);
		expect(sessionStatusLine(session({ state: "working" }))).toBeUndefined();
		expect(sessionStatusLine(undefined)).toBeUndefined();
	});
});

describe("carrying a sealed Workspace on", () => {
	it("drafts the form with the Workspace's project, agent, profile, branch and id", () => {
		expect(
			continueDraft(
				workspace({
					project: "kestrel",
					opened_with: "builder",
					profile: "work",
					checkout: { repositories: [], base: "main", branch: "kestrel/abc" },
				}),
			),
		).toEqual({
			project: "kestrel",
			agent: "builder",
			profile: "work",
			branch: "kestrel/abc",
			continues: "00000000-0000-0000-0000-0000000000ff",
			options: true,
		});
	});
});
