// PROTOTYPE (#492): fixtures shaped like the operator API, for the AI Elements workbench prototype.
import type { HeldMessage, SessionCommand, Usage } from "#/operator/generated";

export const ME = "jack";

export type ToolCall = {
	kind: "tool";
	seq: number;
	callId: string;
	title: string;
	toolKind: "read" | "edit" | "search" | "execute";
	status: "completed" | "failed";
	startedAt: string;
	finishedAt: string;
	input: unknown;
	result: unknown;
	exit?: number;
	language?: string;
};

export type Thought = { kind: "thought"; seq: number; text: string; seconds: number };

export type PlanStep = { content: string; status: "completed" | "in_progress" | "pending" };
export type Plan = { kind: "plan"; seq: number; steps: PlanStep[] };

export type Narrated = ToolCall | Thought | Plan;

export type RunningTool = {
	callId: string;
	title: string;
	toolKind: string;
	status: "pending" | "in_progress";
	startedAt: string;
};

export type ActivityGroup = {
	kind: "activity";
	firstSeq: number;
	lastSeq: number;
	closed: boolean;
	startedAt: string;
	finishedAt: string | null;
	counts: { tools: number; failed: number; thoughts: number; plans: number };
	entries: Narrated[];
};

export type Said = { kind: "said"; seq: number; participant: string; agent: boolean; text: string; at: string };
export type Brief = { kind: "brief"; seq: number; participant: string; text: string; at: string };
export type Notice = { kind: "notice"; seq: number; text: string };

export type FlowItem = ActivityGroup | Said | Brief | Notice;

const t = (clock: string) => `2026-10-04T14:${clock}Z`;

const TEST_SOURCE = `#[tokio::test]
async fn usage_summary_after_trailing_turn() {
    let kestrel = Kestrel::start().await;
    let workspace = kestrel.workspace("usage").await;
    workspace.say("jack", "count to three").await;
    workspace.wait_for_session_ended().await;

    let summary = workspace.summary().await;
    assert_eq!(summary.usage.cost, Some(Cost::usd(0.0042)));
}`;

const CARGO_OUTPUT = `\u001b[1m\u001b[32m   Compiling\u001b[0m kestrel v0.3.0 (/work/crates/kestrel)
\u001b[1m\u001b[32m    Finished\u001b[0m \`test\` profile [unoptimized + debuginfo] target(s) in 41.20s
\u001b[1m\u001b[32m     Running\u001b[0m tests/usage.rs (target/debug/deps/usage-5f0c2e1d)

running 1 test
test usage_summary_after_trailing_turn ... \u001b[31mFAILED\u001b[0m

failures:

---- usage_summary_after_trailing_turn stdout ----
thread 'usage_summary_after_trailing_turn' panicked at crates/kestrel/tests/usage.rs:31:5:
assertion \`left == right\` failed
  left: None
 right: Some(Cost { amount: 0.0042, currency: "USD" })

test result: \u001b[31mFAILED\u001b[0m. 0 passed; 1 failed; 0 ignored; finished in 3.18s`;

const EDIT_DIFF = `@@ -88,9 +88,13 @@ impl Follower {
     fn apply(&mut self, state: SessionState) {
-        if self.ended {
-            return;
-        }
+        if self.ended {
+            // The supervisor reports a Turn's last usage after session_ended.
+            if state.usage.is_some() {
+                self.usage = state.usage;
+            }
+            return;
+        }
         self.tools = state.tools;
         self.usage = state.usage.or(self.usage.take());
     }`;

export const FLOW: FlowItem[] = [
	{ kind: "notice", seq: 1, text: "opencode started" },
	{ kind: "notice", seq: 2, text: "jack joined" },
	{
		kind: "brief",
		seq: 3,
		participant: "jack",
		at: t("02:01"),
		text: "The usage summary test flakes on CI about one run in twenty: `usage_summary_after_trailing_turn` sees `cost: None`. Find out why and fix it without adding a sleep.",
	},
	{
		kind: "activity",
		firstSeq: 4,
		lastSeq: 10,
		closed: true,
		startedAt: t("02:05"),
		finishedAt: t("04:19"),
		counts: { tools: 4, failed: 1, thoughts: 2, plans: 1 },
		entries: [
			{
				kind: "thought",
				seq: 4,
				seconds: 6,
				text: "The test asserts on the usage the follower reports after the Turn ends. If the trailing agent's usage arrives **after** the `session_ended` entry, the follower might drop it.\n\nRead the test and the follower before touching anything.",
			},
			{
				kind: "tool",
				seq: 5,
				callId: "call_01",
				title: "Read crates/kestrel/tests/usage.rs",
				toolKind: "read",
				status: "completed",
				startedAt: t("02:11"),
				finishedAt: t("02:11.120"),
				input: { path: "crates/kestrel/tests/usage.rs" },
				result: TEST_SOURCE,
				language: "rust",
			},
			{
				kind: "tool",
				seq: 6,
				callId: "call_02",
				title: "Read crates/kestrel/src/follow/usage.rs",
				toolKind: "read",
				status: "failed",
				startedAt: t("02:14"),
				finishedAt: t("02:14.040"),
				input: { path: "crates/kestrel/src/follow/usage.rs" },
				result: "No such file or directory (os error 2)",
			},
			{
				kind: "tool",
				seq: 7,
				callId: "call_03",
				title: "Search for usage in crates/kestrel/src",
				toolKind: "search",
				status: "completed",
				startedAt: t("02:16"),
				finishedAt: t("02:16.300"),
				input: { pattern: "usage", path: "crates/kestrel/src" },
				result: "crates/kestrel/src/follow.rs:88\ncrates/kestrel/src/follow.rs:97\ncrates/kestrel/src/session/usage.rs:12",
			},
			{
				kind: "plan",
				seq: 8,
				steps: [
					{ content: "Reproduce the flake under load", status: "completed" },
					{ content: "Find where the follower drops late usage", status: "in_progress" },
					{ content: "Keep usage that trails session_ended", status: "pending" },
					{ content: "Run the test 200 times", status: "pending" },
				],
			},
			{
				kind: "tool",
				seq: 9,
				callId: "call_04",
				title: "cargo test -p kestrel --test usage usage_summary -- --test-threads=1",
				toolKind: "execute",
				status: "completed",
				startedAt: t("03:02"),
				finishedAt: t("03:47"),
				input: { command: "cargo test -p kestrel --test usage usage_summary -- --test-threads=1" },
				result: CARGO_OUTPUT,
				exit: 101,
			},
			{
				kind: "thought",
				seq: 10,
				seconds: 11,
				text: "Reproduced 3 in 50 under `stress`. The follower stops applying `session_state` once it has seen `session_ended`, but the supervisor reports the final usage up to ~50 ms later.",
			},
		],
	},
	{
		kind: "said",
		seq: 11,
		participant: "opencode",
		agent: true,
		at: t("04:20"),
		text: "Found it. The follower stops applying `session_state` once it has seen `session_ended`, but the supervisor reports the Turn's final usage up to ~50 ms **after** that entry. On a slow runner the usage lands late and the summary reads `cost: None`.\n\nThe fix keeps the usage a trailing agent reports:\n\n```rust\nif self.ended && state.usage.is_some() {\n    self.usage = state.usage;\n}\n```\n\nI'll make that change and run the test 200 times.",
	},
	{ kind: "notice", seq: 12, text: "sam joined" },
	{
		kind: "said",
		seq: 13,
		participant: "sam",
		agent: false,
		at: t("04:48"),
		text: "Could it also be the clock? The summary rounds `finished_at` to the second.",
	},
	{
		kind: "said",
		seq: 14,
		participant: "jack",
		agent: false,
		at: t("05:00"),
		text: "Check both, but the ordering one first.",
	},
	{
		kind: "activity",
		firstSeq: 15,
		lastSeq: 17,
		closed: false,
		startedAt: t("05:02"),
		finishedAt: null,
		counts: { tools: 1, failed: 0, thoughts: 1, plans: 0 },
		entries: [
			{
				kind: "thought",
				seq: 15,
				seconds: 4,
				text: "Sam's point is fair, but rounding can't produce `None`. It would only skew the cost. Fix the ordering first, then check the rounding separately.",
			},
			{
				kind: "tool",
				seq: 17,
				callId: "call_05",
				title: "Edit crates/kestrel/src/follow.rs",
				toolKind: "edit",
				status: "completed",
				startedAt: t("05:31"),
				finishedAt: t("05:31.400"),
				input: { path: "crates/kestrel/src/follow.rs" },
				result: EDIT_DIFF,
				language: "diff",
			},
		],
	},
];

export const RUNNING: RunningTool[] = [
	{
		callId: "call_06",
		title: "cargo test -p kestrel --test usage usage_summary -- --test-threads=1",
		toolKind: "execute",
		status: "in_progress",
		startedAt: t("05:41"),
	},
];

export const NOW = Date.parse(t("06:23"));

export const USAGE: Usage = {
	context_used: 84_312,
	context_size: 200_000,
	cost: { amount: 1.87, currency: "USD" },
};

export const HELD: HeldMessage[] = [
	{
		id: 3,
		participant: "sam",
		message: "After this, check whether the CLI follower has the same bug.",
		posted_at: t("05:50"),
		edited_at: null,
	},
	{
		id: 4,
		participant: "jack",
		message: "Don't touch migration 0007; the 0.3 branch shares it.",
		posted_at: t("06:02"),
		edited_at: t("06:04"),
	},
];

export const COMMANDS: SessionCommand[] = [
	{ name: "compact", description: "Summarize the conversation to free context", input_hint: null },
	{ name: "review", description: "Review the current changes", input_hint: "[commit|branch]" },
	{ name: "init", description: "Write an AGENTS.md for this repository", input_hint: null },
	{ name: "undo", description: "Revert the last message's changes", input_hint: null },
];

export type ModelChoice = { id: string; name: string; provider: string };

// The harness offers the same model under two providers, which the spec calls out.
export const MODEL_GROUPS: { provider: string; label: string; models: ModelChoice[] }[] = [
	{
		provider: "anthropic",
		label: "Anthropic",
		models: [
			{ id: "anthropic/claude-opus-5-5", name: "Claude Opus 5.5", provider: "anthropic" },
			{ id: "anthropic/claude-sonnet-5-5", name: "Claude Sonnet 5.5", provider: "anthropic" },
			{ id: "anthropic/claude-haiku-4-5", name: "Claude Haiku 4.5", provider: "anthropic" },
		],
	},
	{
		provider: "openai",
		label: "OpenAI",
		models: [
			{ id: "openai/gpt-5.1", name: "GPT-5.1", provider: "openai" },
			{ id: "openai/gpt-5.1-mini", name: "GPT-5.1 mini", provider: "openai" },
		],
	},
	{
		provider: "google",
		label: "Google",
		models: [{ id: "google/gemini-3-pro", name: "Gemini 3 Pro", provider: "google" }],
	},
	{
		provider: "opencode",
		label: "OpenCode Zen",
		models: [
			{ id: "opencode/claude-opus-5-5", name: "Claude Opus 5.5", provider: "opencode" },
			{ id: "opencode/claude-sonnet-5-5", name: "Claude Sonnet 5.5", provider: "opencode" },
			{ id: "opencode/big-pickle", name: "Big Pickle", provider: "opencode" },
		],
	},
];

export const SESSION = {
	title: "Fix the flaky usage summary test",
	workspace: "flaky-usage-test",
	project: "kestrel",
	branch: "jack/flaky-usage",
	phase: "Working",
	agent: "opencode",
	model: "anthropic/claude-opus-5-5",
	watching: "watching sam · 1 anonymous",
};

export const WORKSPACES = [
	{ name: "flaky-usage-test", project: "kestrel", branch: "jack/flaky-usage", phase: "Working", current: true },
	{ name: "cli-completions", project: "kestrel", branch: "sam/completions", phase: "Waiting for you", current: false },
	{ name: "compose-up-docs", project: "kestrel", branch: "docs/compose", phase: "Queued", current: false },
	{ name: "relay-spike", project: "kestrel-relay", branch: "main", phase: "Ended", current: false },
];

export const QUEUED_SESSIONS = [
	{ workspace: "compose-up-docs", participant: "sam", brief: "Rewrite the compose quickstart for 0.4", position: 1 },
	{ workspace: "relay-spike", participant: "jack", brief: "Measure relay latency from a laptop", position: 2 },
];

export const COMMITS = [
	{
		hash: "a1b2c3d4e5f6",
		message: "Keep the usage a trailing agent reports",
		author: "opencode",
		at: t("05:32"),
		files: [{ path: "crates/kestrel/src/follow.rs", status: "modified" as const, added: 6, removed: 2 }],
	},
	{
		hash: "9f8e7d6c5b4a",
		message: "Reproduce the late-usage race in a test",
		author: "opencode",
		at: t("03:58"),
		files: [{ path: "crates/kestrel/tests/usage.rs", status: "modified" as const, added: 12, removed: 0 }],
	},
];

export const UNPUBLISHED = {
	repository: "kestrel",
	files: [
		{ path: "crates/kestrel/src/follow.rs", added: 6, removed: 2 },
		{ path: "crates/kestrel/tests/usage.rs", added: 12, removed: 0 },
	],
	diff: `diff --git a/crates/kestrel/src/follow.rs b/crates/kestrel/src/follow.rs
--- a/crates/kestrel/src/follow.rs
+++ b/crates/kestrel/src/follow.rs
${EDIT_DIFF}`,
};

export const FILES: Record<string, string> = {
	"crates/kestrel/src/follow.rs": `use crate::session::{SessionState, Usage};

pub struct Follower {
    ended: bool,
    tools: Vec<RunningTool>,
    usage: Option<Usage>,
}

impl Follower {
    fn apply(&mut self, state: SessionState) {
        if self.ended {
            // The supervisor reports a Turn's last usage after session_ended.
            if state.usage.is_some() {
                self.usage = state.usage;
            }
            return;
        }
        self.tools = state.tools;
        self.usage = state.usage.or(self.usage.take());
    }
}`,
	"crates/kestrel/src/lib.rs": "pub mod follow;\npub mod session;\n",
	"crates/kestrel/src/session/mod.rs": "mod usage;\npub use usage::Usage;\n",
	"crates/kestrel/src/session/usage.rs": "#[derive(Clone, Debug, PartialEq)]\npub struct Usage {\n    pub context_used: u64,\n    pub context_size: u64,\n    pub cost: Option<Cost>,\n}\n",
	"crates/kestrel/tests/usage.rs": TEST_SOURCE,
	"Cargo.toml": '[workspace]\nmembers = ["crates/*"]\nresolver = "3"\n',
	"README.md": "# kestrel\n\nRun coding agents for a team.\n",
};
