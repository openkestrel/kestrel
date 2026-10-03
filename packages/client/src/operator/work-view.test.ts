import { describe, expect, it } from "vitest";
import type { Session } from "./generated";
import { sessionPhase } from "./session-state";
import { delivered, type Delivered } from "./transcript";
import { transport } from "./transport";
import { readFile } from "./work-queries";
import {
	breadcrumbs,
	changesText,
	childPath,
	commitsText,
	fileKindLabel,
	joinedParticipants,
	optionSummary,
	parentPath,
	pushedText,
	scopeLabel,
	sessionContinuity,
	sessionOutcome,
	stashedText,
	untrackedText,
	usageText,
} from "./work-view";

function session(overrides: Partial<Session> = {}): Session {
	return {
		id: "00000000-0000-0000-0000-000000000001",
		name: "calm-river-abcdefgh",
		workspace: "00000000-0000-0000-0000-0000000000ff",
		state: "working",
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
		units: [],
		message_buffering: false,
		thought_buffering: false,
		last_activity_at: null,
		...overrides,
	};
}

function joined(seq: number, participant: string): Delivered {
	return delivered({
		seq,
		appended_at: "2026-09-30T10:00:00Z",
		kind: "shared_state",
		session_id: null,
		entry: { type: "participant_joined", participant },
	});
}

describe("work readings", () => {
	it("names each kind of work separately", () => {
		expect(pushedText("abcdef1234567890")).toBe("pushed abcdef12");
		expect(pushedText(null)).toBe("nothing pushed");
		expect(changesText({ files: 2, added: 10, removed: 3 })).toBe("2 files +10 −3");
		expect(changesText({ files: 1, added: 0, removed: 0 })).toBe("1 file +0 −0");
		expect(commitsText({ commits: 3, added: 8, removed: 1 })).toBe("3 commits +8 −1");
		expect(commitsText({ commits: 1, added: 0, removed: 0 })).toBe("1 commit +0 −0");
		expect(untrackedText(1)).toBe("1 file");
		expect(untrackedText(4)).toBe("4 files");
		expect(stashedText(1)).toBe("1 stash");
		expect(stashedText(2)).toBe("2 stashes");
	});
});

describe("diff scopes", () => {
	it("names the published scopes and a commit", () => {
		expect(scopeLabel("unpublished")).toBe("Unpublished");
		expect(scopeLabel("changed")).toBe("Changed");
		expect(scopeLabel("staged")).toBe("Staged");
		expect(scopeLabel("commit:abcdef1234567890")).toBe("Commit abcdef12");
	});
});

describe("the file browser's paths", () => {
	it("moves between a repository root and its children", () => {
		expect(parentPath("kestrel/src/main.rs")).toBe("kestrel/src");
		expect(parentPath("kestrel")).toBe("");
		expect(childPath("kestrel", "src")).toBe("kestrel/src");
		expect(childPath("", "kestrel")).toBe("kestrel");
		expect(breadcrumbs("kestrel/src")).toEqual([
			{ label: "repositories", path: "" },
			{ label: "kestrel", path: "kestrel" },
			{ label: "src", path: "kestrel/src" },
		]);
	});

	it("kinds an entry", () => {
		expect(fileKindLabel({ name: "src", kind: "directory" })).toBe("directory");
		expect(fileKindLabel({ name: "new.rs", kind: "file", git: "untracked" })).toBe("untracked");
		expect(fileKindLabel({ name: "main.rs", kind: "file", git: "tracked" })).toBe("tracked");
	});
});

describe("a Session's reading", () => {
	it("names its phase, outcome, continuity, options and usage", () => {
		expect(sessionPhase(session({ state: "unbriefed", preparing: "cloning" }))).toBe("Preparing");
		expect(sessionPhase(session({ state: "ended" }))).toBe("Ended");
		expect(
			sessionOutcome(
				session({
					state: "ended",
					exit: { status: "failed", because: "the Session lost ACP continuity" },
				}),
			),
		).toBe("failed: the Session lost ACP continuity");
		expect(sessionOutcome(session({ state: "working" }))).toBeUndefined();
		expect(
			sessionContinuity(
				session({
					instance: "local-1",
					supervisor: "1.2.3",
					connected_at: new Date(Date.now() - 120_000).toISOString(),
				}),
			),
		).toContain("instance local-1 · supervisor 1.2.3 · connected 2 minutes ago");
		expect(sessionContinuity(session())).toBe("no Instance");
		expect(
			optionSummary(
				session({
					options: [
						{
							id: "model",
							name: "Model",
							description: null,
							category: "model",
							kind: "select",
							current: "sonnet",
							values: [],
							groups: [],
							warns_cache: false,
						},
					],
				}),
			),
		).toEqual(["Model: sonnet"]);
		expect(
			usageText(session({ usage: { context_used: 1200, context_size: 200_000, cost: null } })),
		).toBe("1.2k/200.0k context");
	});
});

describe("the People reading", () => {
	it("lists joined Participants once, in order", () => {
		const entries = [joined(1, "builder"), joined(2, "jack"), joined(3, "builder")];

		expect(joinedParticipants(entries)).toEqual(["builder", "jack"]);
	});
});

describe("a file read", () => {
	it("reads an inline text answer", async () => {
		const operator = transport(async () =>
			Response.json({ path: "kestrel/README.md", text: "# kestrel" }),
		);

		expect(await readFile(operator, "acme", "brave-otter", "kestrel/README.md", false)).toEqual({
			kind: "text",
			path: "kestrel/README.md",
			text: "# kestrel",
		});
	});

	it("reads raw bytes that decode as text, and reports the rest by size", async () => {
		const text = transport(
			async () =>
				new Response(new TextEncoder().encode("plain text"), {
					headers: { "content-type": "application/octet-stream" },
				}),
		);
		expect(await readFile(text, "acme", "brave-otter", "kestrel/notes.txt", true)).toEqual({
			kind: "text",
			path: "kestrel/notes.txt",
			text: "plain text",
		});

		const binary = transport(
			async () =>
				new Response(new Uint8Array([0, 1, 2, 3]), {
					headers: { "content-type": "application/octet-stream" },
				}),
		);
		expect(await readFile(binary, "acme", "brave-otter", "kestrel/logo.png", false)).toEqual({
			kind: "bytes",
			bytes: 4,
		});
	});
});
