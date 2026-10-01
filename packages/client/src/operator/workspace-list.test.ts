import { describe, expect, it } from "vitest";
import type { Queue, QueueReason, Session, Workspace, WorkspaceWork } from "./generated";
import { currentUnit, needsAttention, phaseLabel, phaseOf } from "./session-state";
import {
	changedWork,
	composeRow,
	learnedPullRequest,
	order,
	pullRequestsUnavailable,
	reasonText,
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
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

function queue(overrides: Partial<Queue> = {}): Queue {
	return {
		work_role: null,
		active_work: { limit: null, occupied: 0, occupants: [], elsewhere: 0 },
		instances: { limit: null, count: 0, counted: [] },
		queued: [],
		waiting: [],
		unbriefed: [],
		...overrides,
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
			composeRow(
				workspace({ id: "q2", name: "q2", opened_at: "2026-09-30T10:04:00Z" }),
				[session({ workspace: "q2", enqueued_at: "2026-09-30T10:04:00Z" })],
				queue({
					queued: [
						{
							position: 2,
							name: "q2",
							workspace: "q2",
							agent: "builder",
							reasons: [],
							enqueued_at: "2026-09-30T10:04:00Z",
						},
					],
				}),
			),
			composeRow(
				workspace({ id: "working", name: "working" }),
				[session({ workspace: "working", state: "working" })],
				undefined,
			),
			composeRow(
				workspace({ id: "attention", name: "attention" }),
				[session({ workspace: "attention", state: "unreachable" })],
				undefined,
			),
			composeRow(
				workspace({ id: "waiting", name: "waiting" }),
				[session({ workspace: "waiting", state: "waiting" })],
				undefined,
			),
			composeRow(
				workspace({ id: "q1", name: "q1", opened_at: "2026-09-30T10:03:00Z" }),
				[session({ workspace: "q1", enqueued_at: "2026-09-30T10:03:00Z" })],
				queue({
					queued: [
						{
							position: 1,
							name: "q1",
							workspace: "q1",
							agent: "builder",
							reasons: [],
							enqueued_at: "2026-09-30T10:03:00Z",
						},
					],
				}),
			),
		];

		expect(order(rows).map((row) => row.workspace.name)).toEqual([
			"attention",
			"working",
			"waiting",
			"q1",
			"q2",
		]);
	});

	it("keeps a queued Session's place from the queue, and a waiting Session out of it", () => {
		const row = composeRow(
			workspace(),
			[session({ state: "waiting" })],
			queue({
				waiting: [
					{
						name: "calm-river-abcdefgh",
						workspace: "11111111-1111-1111-1111-111111111111",
						agent: "builder",
						pending_since: "2026-09-30T10:01:00Z",
						reasons: [{ kind: "ahead", sessions: ["calm-river-abcdefgh"] }],
						enqueued_at: "2026-09-30T10:00:00Z",
					},
				],
			}),
		);

		expect(row.position).toBeNull();
		expect(row.pendingSince).toBe("2026-09-30T10:01:00Z");
		expect(phaseOf(row)).toBe("waiting");
		expect(phaseLabel(row)).toBe("Waiting");
	});
});

describe("attention", () => {
	it("is a held Instance, a lost supervisor or a failed Session", () => {
		expect(needsAttention(composeRow(workspace({ held: "unpublished work" }), [], undefined))).toBe(
			true,
		);
		expect(
			needsAttention(composeRow(workspace(), [session({ state: "unreachable" })], undefined)),
		).toBe(true);
		expect(
			needsAttention(
				composeRow(
					workspace(),
					[session({ state: "ended", exit: { status: "failed", because: "it broke" } })],
					undefined,
				),
			),
		).toBe(true);
		expect(
			needsAttention(
				composeRow(
					workspace(),
					[session({ state: "ended", exit: { status: "succeeded" } })],
					undefined,
				),
			),
		).toBe(false);
	});
});

describe("a row's phase", () => {
	it("names preparing, working, waiting and queued", () => {
		expect(phaseLabel(composeRow(workspace(), [session({ state: "unbriefed" })], undefined))).toBe(
			"Preparing",
		);
		expect(phaseLabel(composeRow(workspace(), [session({ state: "working" })], undefined))).toBe(
			"Working",
		);
		expect(phaseLabel(composeRow(workspace(), [session({ state: "queued" })], undefined))).toBe(
			"Queued",
		);
		expect(
			phaseLabel(
				composeRow(
					workspace(),
					[session({ state: "queued" })],
					queue({
						queued: [
							{
								position: 3,
								name: "calm-river-abcdefgh",
								workspace: "11111111-1111-1111-1111-111111111111",
								agent: "builder",
								reasons: [],
								enqueued_at: "2026-09-30T10:00:00Z",
							},
						],
					}),
				),
			),
		).toBe("Queued #3");
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
							status: "in_progress",
							started_at: "2026-09-30T10:00:00Z",
						},
					],
				}),
			),
		).toBe("cargo test");
		expect(currentUnit(session({ state: "unbriefed", preparing: "cloning" }))).toBe("cloning");
		expect(currentUnit(session({ state: "working", thought_buffering: true }))).toBe("thinking");
		expect(currentUnit(session({ state: "working", message_buffering: true }))).toBe("writing");
		expect(currentUnit(session({ state: "waiting" }))).toBeUndefined();
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

	it("joins a waiting row's reasons, and falls back to held input", () => {
		const held: QueueReason = { kind: "active_work_slots", limit: 1 };
		const row = composeRow(
			workspace(),
			[session({ state: "waiting" })],
			queue({
				waiting: [
					{
						name: "calm-river-abcdefgh",
						workspace: "11111111-1111-1111-1111-111111111111",
						agent: "builder",
						pending_since: "2026-09-30T10:01:00Z",
						reasons: [held],
						enqueued_at: "2026-09-30T10:00:00Z",
					},
				],
			}),
		);
		expect(waitingText(row)).toBe("every Active-Work Slot is occupied (1)");

		const input = composeRow(
			workspace(),
			[session({ state: "waiting" })],
			queue({
				waiting: [
					{
						name: "calm-river-abcdefgh",
						workspace: "11111111-1111-1111-1111-111111111111",
						agent: "builder",
						pending_since: new Date(Date.now() - 120_000).toISOString(),
						reasons: [],
						enqueued_at: "2026-09-30T10:00:00Z",
					},
				],
			}),
		);
		expect(waitingText(input)).toBe("input held since 2m ago");
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
