import { describe, expect, it } from "vitest";
import type {
	QueueStanding,
	Session,
	Workspace,
	WorkspaceListed,
	WorkspaceWork,
} from "./generated";
import { currentUnit, needsAttention, phaseLabel, phaseOf } from "./session-state";
import {
	changedWork,
	learnedPullRequest,
	order,
	pullRequestsUnavailable,
	waitingText,
	workNote,
} from "./workspace-list";

function workspace(overrides: Partial<Workspace> = {}): Workspace {
	return {
		id: "11111111-1111-1111-1111-111111111111",
		name: "brave-otter-abcdefgh",
		organization: "acme",
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: { repositories: [], base: "main", branch: "kestrel/1111" },
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

function session(overrides: Partial<Session> = {}): Session {
	return {
		id: "22222222-2222-2222-2222-222222222222",
		name: "calm-river-abcdefgh",
		workspace: "11111111-1111-1111-1111-111111111111",
		state: "queued",
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
		depends_on: [],
		tools: [],
		units: [],
		message_buffering: false,
		thought_buffering: false,
		last_activity_at: null,
		...overrides,
	};
}

function listed(
	overrides: Partial<Workspace>,
	latest: Session | null,
	queue: Partial<QueueStanding> | null = null,
): WorkspaceListed {
	return {
		...workspace(overrides),
		session: latest,
		queue: queue && { position: null, reasons: [], pending_since: null, ...queue },
	};
}

function pull(number: number, updated_at: string) {
	return {
		repository: "kestrel",
		number,
		url: `https://github.com/openkestrel/kestrel/pull/${number}`,
		title: `pull ${number}`,
		state: "open" as const,
		head_branch: "kestrel/1111",
		head_revision: "abcdef",
		updated_at,
		event: "33333333-3333-3333-3333-333333333333",
	};
}

describe("the Workspace list order", () => {
	it("puts attention, then working, waiting and queued FIFO", () => {
		const rows = [
			listed(
				{ id: "q2", name: "q2", opened_at: "2026-09-30T10:04:00Z" },
				session({ enqueued_at: "2026-09-30T10:04:00Z" }),
				{ position: 2 },
			),
			listed({ id: "working", name: "working" }, session({ state: "working" })),
			listed({ id: "attention", name: "attention" }, session({ state: "unreachable" })),
			listed({ id: "waiting", name: "waiting" }, session({ state: "waiting" })),
			listed(
				{ id: "q1", name: "q1", opened_at: "2026-09-30T10:03:00Z" },
				session({ enqueued_at: "2026-09-30T10:03:00Z" }),
				{ position: 1 },
			),
		];

		expect(order(rows).map((row) => row.name)).toEqual([
			"attention",
			"working",
			"waiting",
			"q1",
			"q2",
		]);
	});
});

describe("attention", () => {
	it("is a held Instance, a lost supervisor or a failed Session", () => {
		expect(needsAttention(listed({ held: "unpublished work" }, null))).toBe(true);
		expect(needsAttention(listed({}, session({ state: "unreachable" })))).toBe(true);
		expect(
			needsAttention(
				listed({}, session({ state: "ended", exit: { status: "failed", because: "it broke" } })),
			),
		).toBe(true);
		expect(
			needsAttention(listed({}, session({ state: "ended", exit: { status: "succeeded" } }))),
		).toBe(false);
	});
});

describe("a row's phase", () => {
	it("names preparing, working, waiting and queued", () => {
		expect(phaseLabel(listed({}, session({ state: "unbriefed" })))).toBe("Preparing");
		expect(phaseLabel(listed({}, session({ state: "working" })))).toBe("Working");
		expect(phaseLabel(listed({}, session({ state: "queued" })))).toBe("Queued");
		expect(phaseLabel(listed({}, session({ state: "queued" }), { position: 3 }))).toBe("Queued #3");
		const waiting = listed({}, session({ state: "waiting" }), {
			pending_since: "2026-09-30T10:01:00Z",
		});
		expect(phaseOf(waiting)).toBe("waiting");
		expect(phaseLabel(waiting)).toBe("Waiting");
		expect(phaseLabel(listed({}, null))).toBe("Open");
	});
});

describe("the current unit", () => {
	it("is the running tool, the preparing step or what the Session is writing", () => {
		expect(
			currentUnit(
				session({
					state: "working",
					tools: [
						{
							call_id: "call",
							title: "cargo test",
							tool_kind: "execute",
							status: "in_progress",
							started_at: "2026-09-30T10:00:00Z",
						},
					],
				}),
			),
		).toBe("cargo test");
		expect(currentUnit(session({ state: "unbriefed", preparing: "cloning" }))).toBe(
			"cloning the checkout",
		);
		expect(currentUnit(session({ state: "working", thought_buffering: true }))).toBe("thinking");
		expect(currentUnit(session({ state: "working", message_buffering: true }))).toBe("writing");
		expect(currentUnit(session({ state: "waiting" }))).toBeUndefined();
	});
});

describe("queue reasons", () => {
	it("joins a waiting row's reasons, and falls back to held input", () => {
		const row = listed({}, session({ state: "waiting" }), {
			pending_since: "2026-09-30T10:01:00Z",
			reasons: [{ kind: "active_work_slots", limit: 1 }],
		});
		expect(waitingText(row)).toBe("every Active-Work Slot is occupied (1)");

		const input = listed({}, session({ state: "waiting" }), {
			pending_since: new Date(Date.now() - 120_000).toISOString(),
		});
		expect(waitingText(input)).toBe("input held since 2 minutes ago");
	});
});

describe("the work summary", () => {
	it("sums changed and staged lines across repositories", () => {
		const work: WorkspaceWork = {
			state: "reported",
			reported_at: "2026-09-30T10:00:00Z",
			repositories: [
				{
					repository: "kestrel",
					git: "read",
					branch: "kestrel/1111",
					changed: { files: 2, added: 10, removed: 3 },
					staged: { files: 1, added: 4, removed: 1 },
					committed: { commits: 1, added: 5, removed: 0 },
					pushed: null,
					untracked: 2,
					stashed: 0,
				},
				{
					repository: "skills",
					git: "unreadable",
					because: "no such directory",
				},
			],
		};

		expect(changedWork(work)).toEqual({
			repositories: 2,
			files: 3,
			added: 14,
			removed: 4,
			untracked: 2,
			unreadable: 1,
		});
		expect(
			changedWork({ state: "no_instance", branch: "main", pull_request: null }),
		).toBeUndefined();
	});

	it("says why a row has no work reading, and how fresh one is", () => {
		expect(workNote({ state: "no_instance", branch: "main", pull_request: null })).toBe(
			"no Instance",
		);
		expect(workNote({ state: "not_answering", message: "the Instance isn't answering" })).toBe(
			"the Instance isn't answering",
		);
		expect(
			workNote({ state: "reported", repositories: [], reported_at: new Date().toISOString() }),
		).toBe("reported just now");
	});
});

describe("learned pull requests", () => {
	it("picks the most recently updated one, and says when none can be learned", () => {
		expect(
			learnedPullRequest(
				workspace({
					pull_requests: [
						{
							repository: "kestrel",
							availability: "available",
							known: [pull(1, "2026-09-30T09:00:00Z"), pull(2, "2026-09-30T10:00:00Z")],
						},
					],
				}),
			)?.number,
		).toBe(2);

		expect(
			learnedPullRequest(
				workspace({
					pull_requests: [{ repository: "kestrel", availability: "unavailable", known: null }],
				}),
			),
		).toBeUndefined();
		expect(
			pullRequestsUnavailable(
				workspace({
					pull_requests: [{ repository: "kestrel", availability: "unavailable", known: null }],
				}),
			),
		).toBe(true);
	});
});
