import { describe, expect, it } from "vitest";
import type { Presence, Session, Usage, Workspace } from "./generated";
import {
	commandLine,
	continuityLine,
	followersLine,
	interruptingLabel,
	mayInterrupt,
	modelLine,
	optionCurrent,
	pendingLine,
	sessionTitle,
	stateLabel,
	usageLine,
} from "./session-view";

function session(overrides: Partial<Session> = {}): Session {
	return {
		id: "22222222-2222-2222-2222-222222222222",
		name: "calm-river",
		workspace: "11111111-1111-1111-1111-111111111111",
		state: "waiting",
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
		changing_options: [],
		commands: [],
		interrupting: null,
		enqueued_at: "2026-09-30T10:00:00Z",
		started_at: null,
		ended_at: null,
		lease_expires_at: null,
		connected_at: null,
		supervisor_version: null,
		usage: null,
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

function workspace(overrides: Partial<Workspace> = {}): Workspace {
	return {
		id: "11111111-1111-1111-1111-111111111111",
		name: "kind-sparrow",
		organization: "acme",
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: { repositories: [], base: "main", branch: "kestrel/kind-sparrow" },
		instance: null,
		held: null,
		held_messages: [],
		correlation: null,
		state: "open",
		opened_at: "2026-09-30T10:00:00Z",
		last_active_at: "2026-09-30T10:00:00Z",
		sealed_at: null,
		continues: null,
		started_by: null,
		continued_by: [],
		pull_requests: [],
		unfinished_session: null,
		...overrides,
	};
}

describe("the header's title and state", () => {
	it("is the harness's title, the Session's own name before one is said, or the Workspace's", () => {
		expect(sessionTitle(session({ title: "the scripted conversation" }), workspace())).toBe(
			"the scripted conversation",
		);
		expect(sessionTitle(session(), workspace())).toBe("calm-river");
		expect(sessionTitle(undefined, workspace())).toBe("kind-sparrow");
	});

	it("names the step an unbriefed Session is preparing on", () => {
		expect(stateLabel(session({ state: "unbriefed", preparing: "harness_ready" }))).toBe(
			"unbriefed · harness ready",
		);
		expect(stateLabel(session({ state: "working" }))).toBe("working");
		expect(stateLabel(undefined)).toBe("no session");
	});
});

describe("the header's model line", () => {
	it("tells the requested model from the one the harness reports", () => {
		expect(modelLine(session({ model: "mini", worked_model: "max" }))).toBe(
			"requested mini · running max",
		);
		expect(modelLine(session({ model: "mini", worked_model: "mini" }))).toBe("model mini");
		expect(modelLine(session())).toBe("model the harness's default");
	});
});

describe("the header's continuity line", () => {
	it("names the Workspaces it continues and the ones continuing it", () => {
		const earlier = workspace({ id: "aaaa", name: "early-bird" });
		const later = workspace({ id: "bbbb", name: "late-lark" });
		expect(
			continuityLine(workspace({ continues: "aaaa", continued_by: ["bbbb"] }), [earlier, later]),
		).toBe("continues early-bird · continued by late-lark");
		expect(continuityLine(workspace(), [])).toBeUndefined();
	});
});

describe("the header's usage, followers and commands", () => {
	it("reads the context, its size and what it cost", () => {
		const usage: Usage = {
			context_used: 1_200,
			context_size: 200_000,
			cost: { amount: 0.42, currency: "USD" },
		};
		expect(usageLine(usage)).toBe("1,200 of 200,000 tokens · 0.42 USD");
		expect(usageLine(null)).toBeUndefined();
	});

	it("names the followers watching and counts the anonymous", () => {
		const presence: Presence = { named: ["jack", "jill"], anonymous: 2 };
		expect(followersLine(presence)).toBe("watching jack, jill · 2 anonymous");
		expect(followersLine({ named: [], anonymous: 0 })).toBe("no one else is watching");
		expect(followersLine(undefined)).toBeUndefined();
	});

	it("offers a command by its input hint", () => {
		expect(commandLine({ name: "compact", description: "Compact", input_hint: "/compact" })).toBe(
			"/compact",
		);
	});
});

describe("an option and a pending change", () => {
	it("reads a boolean as on or off", () => {
		expect(
			optionCurrent({
				id: "verbose",
				name: "Verbose",
				description: null,
				category: "_scripted",
				kind: "boolean",
				current: true,
				values: [],
				groups: [],
				warns_cache: false,
			}),
		).toBe("on");
	});

	it("says who is changing what", () => {
		expect(
			pendingLine({
				option: "model",
				category: "model",
				value: "max",
				participant: "jack",
			}),
		).toBe("jack is changing model to max");
	});
});

describe("interrupting", () => {
	it("is offered only while a Turn is working, and says who asked", () => {
		expect(mayInterrupt(session({ state: "working" }))).toBe(true);
		expect(mayInterrupt(session({ state: "waiting" }))).toBe(false);
		expect(
			interruptingLabel(
				session({
					state: "working",
					interrupting: { participant: "jack", requested_at: "2026-09-30T10:00:00Z" },
				}),
			),
		).toBe("jack asked this Turn to stop");
	});
});
