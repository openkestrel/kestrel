import { describe, expect, it } from "vitest";
import type { Presence, Session, TranscriptSessionState, Usage, Workspace } from "./generated";
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
	trailingOf,
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
		diagnostic: null,
		instance: null,
		supervisor: null,
		agent: "builder",
		harness: "opencode",
		model: null,
		mode: null,
		thought_level: null,
		worked_model: null,
		sign_in_method: null,
		supplied: [],
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
		depends_on: [],
		tools: [],
		units: [],
		message_buffering: false,
		thought_buffering: false,
		last_activity_at: null,
		observation: { availability: "unavailable", last: null },
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

describe("a trailing Session", () => {
	const observed = { availability: "current", observed_at: "2026-09-30T10:00:00Z" } as const;
	const build = {
		call_id: "call-1",
		title: "cargo build",
		tool_kind: "execute",
		status: "in_progress",
		started_at: "2026-09-30T09:59:00Z",
	};
	const reviewer = {
		id: "unit-1",
		kind: "subagent",
		title: "review the diff",
		started_at: "2026-09-30T09:59:30Z",
	} as const;
	const watcher = {
		id: "unit-2",
		kind: "background_task",
		title: "bun run dev",
		started_at: "2026-09-30T09:59:40Z",
	} as const;

	function live(overrides: Partial<TranscriptSessionState> = {}): TranscriptSessionState {
		return {
			session_id: "22222222-2222-2222-2222-222222222222",
			tools: [],
			units: [],
			message_buffering: false,
			thought_buffering: false,
			usage: null,
			observation: observed,
			...overrides,
		};
	}

	it("names the open tools and Adapter Units a current observation lists", () => {
		expect(
			trailingOf(
				session({
					state: "trailing",
					observation: observed,
					tools: [build],
					units: [reviewer, watcher],
				}),
				undefined,
			),
		).toEqual({
			observed: "open",
			work: [
				{ key: "tool:call-1", title: "cargo build", kind: "tool" },
				{ key: "unit:unit-1", title: "review the diff", kind: "subagent" },
				{ key: "unit:unit-2", title: "bun run dev", kind: "background task" },
			],
		});
	});

	it("only checks that work has settled when a current observation lists nothing", () => {
		expect(trailingOf(session({ state: "trailing", observation: observed }), undefined)).toEqual({
			observed: "settling",
		});
		expect(
			trailingOf(session({ state: "trailing", observation: observed }), live({ tools: [] })),
		).toEqual({ observed: "settling" });
	});

	it("never reads an unavailable observation's empty lists as quiet", () => {
		const unseen = session({
			state: "trailing",
			observation: { availability: "unavailable", last: null },
		});
		expect(trailingOf(unseen, undefined)).toEqual({ observed: "unavailable", last: null });
		expect(
			trailingOf(
				unseen,
				live({
					observation: {
						availability: "unavailable",
						last: { observed_at: "2026-09-30T09:50:00Z", tools: [build], units: [] },
					},
				}),
			),
		).toEqual({
			observed: "unavailable",
			last: {
				observedAt: "2026-09-30T09:50:00Z",
				work: [{ key: "tool:call-1", title: "cargo build", kind: "tool" }],
			},
		});
	});

	it("takes the follow's snapshot over the Session read, once it is about this Session", () => {
		const read = session({ state: "trailing", observation: observed, tools: [build] });
		expect(
			trailingOf(read, live({ observation: { availability: "unavailable", last: null } })),
		).toEqual({ observed: "unavailable", last: null });
		expect(trailingOf(read, live({ session_id: null }))).toMatchObject({ observed: "open" });
		expect(trailingOf(read, live({ session_id: "another" }))).toMatchObject({ observed: "open" });
	});

	it("explains nothing for a Session that is not trailing", () => {
		expect(
			trailingOf(session({ state: "working", observation: observed }), live()),
		).toBeUndefined();
		expect(trailingOf(undefined, live())).toBeUndefined();
	});
});
