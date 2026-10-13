import type { Activity, Entry, Exit, PayloadReference, PlanStep, RunningTool } from "./generated";
import type { Delivered } from "./transcript";

export type FlowItem =
	| { kind: "entry"; entry: Delivered }
	| { kind: "activity"; activity: Activity };

export function flow(entries: Delivered[], activities: Activity[]): FlowItem[] {
	const items: FlowItem[] = [
		...entries.map((entry): FlowItem => ({ kind: "entry", entry })),
		...activities.map((activity): FlowItem => ({ kind: "activity", activity })),
	];
	return items.toSorted((one, other) => sequence(one) - sequence(other));
}

function sequence(item: FlowItem): number {
	return item.kind === "entry" ? item.entry.seq : item.activity.first_seq;
}

export type ActivitySummary = {
	counts: string;
	outcome: string | undefined;
	duration: string | undefined;
	name: string;
};

// An Activity is undefined while its first calls are still running and nothing is recorded yet.
export function activitySummary(activity: Activity | undefined, live: boolean): ActivitySummary {
	const counts = activity ? activityCounts(activity) : "";
	const outcome = activity?.anomaly ? "interrupted or unresolved" : undefined;
	const duration = activity && !live ? activityDuration(activity) : undefined;
	return {
		counts,
		outcome,
		duration,
		name: phrase("Activity", live && "Working", counts, outcome, duration),
	};
}

// Inline parts read as one run-together word to a screen reader unless the name separates them.
export function phrase(...parts: (string | false | undefined)[]): string {
	return parts.filter((part) => part !== false && part !== undefined && part !== "").join(", ");
}

function activityCounts(activity: Activity): string {
	const counts = activity.counts;
	const parts: string[] = [];
	if (counts.tool_calls > 0)
		parts.push(`${counts.tool_calls} ${counts.tool_calls === 1 ? "tool call" : "tool calls"}`);
	if (counts.failed_calls > 0) parts.push(`${counts.failed_calls} failed`);
	if (counts.thoughts > 0)
		parts.push(`${counts.thoughts} ${counts.thoughts === 1 ? "thought" : "thoughts"}`);
	if (counts.plans > 0) parts.push(`${counts.plans} ${counts.plans === 1 ? "plan" : "plans"}`);
	if (counts.tombstones > 0) parts.push(`${counts.tombstones} expired`);
	return parts.join(", ");
}

function activityDuration(activity: Activity): string | undefined {
	if (!activity.started_at || !activity.finished_at) return undefined;
	return elapsed(Date.parse(activity.finished_at) - Date.parse(activity.started_at));
}

export type ActivityStep =
	| { kind: "entry"; key: string; entry: Delivered }
	| { kind: "running"; key: string; tool: RunningTool };

// A call keeps one key from running to recorded, so its row is replaced where it stands.
export function activitySteps(entries: Delivered[], running: RunningTool[]): ActivityStep[] {
	const recorded = new Set<string>();
	const steps = entries.map((entry): ActivityStep => {
		const call = entry.entry.type === "tool_call" ? entry.entry.call_id : undefined;
		if (call === undefined || recorded.has(call)) {
			return { kind: "entry", key: `seq:${entry.seq}`, entry };
		}
		recorded.add(call);
		return { kind: "entry", key: `call:${call}`, entry };
	});
	const open = running
		.filter((tool) => !recorded.has(tool.call_id))
		.toSorted((one, other) => Date.parse(one.started_at) - Date.parse(other.started_at));
	return [
		...steps,
		...open.map((tool): ActivityStep => ({ kind: "running", key: `call:${tool.call_id}`, tool })),
	];
}

export type ActivityRead =
	| { kind: "reading"; delayed: boolean; known: Delivered[] }
	| { kind: "failed"; error: unknown; known: Delivered[]; retrying: boolean }
	| { kind: "empty" }
	| { kind: "read"; entries: Delivered[] };

export type ActivityQueryState = {
	data: Delivered[] | undefined;
	error: unknown;
	fetching: boolean;
};

// `known` is the last range that was read, which a grown Activity's next read has not replaced yet.
export function activityRead(
	{ data, error, fetching }: ActivityQueryState,
	known: Delivered[],
	delayed: boolean,
): ActivityRead {
	if (error !== null && error !== undefined) {
		return { kind: "failed", error, known: data ?? known, retrying: fetching };
	}
	if (data === undefined) return { kind: "reading", delayed, known };
	return data.length === 0 ? { kind: "empty" } : { kind: "read", entries: data };
}

export function elapsed(milliseconds: number): string | undefined {
	if (!Number.isFinite(milliseconds) || milliseconds < 0) return undefined;
	const seconds = milliseconds / 1000;
	if (seconds < 1) return `${Math.max(1, Math.round(milliseconds))}ms`;
	if (seconds < 10) return `${seconds.toFixed(1)}s`;
	if (seconds < 60) return `${Math.round(seconds)}s`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m ${Math.round(seconds % 60)}s`;
	return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function at(timestamp: string | null | undefined): string | undefined {
	if (!timestamp) return undefined;
	const date = new Date(timestamp);
	return Number.isNaN(date.getTime())
		? undefined
		: date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export type ToolState = "running" | "completed" | "failed" | "interrupted" | "unresolved";

export function toolState(status: string, closingReason?: string | null): ToolState {
	switch (closingReason) {
		case "interrupted":
			return "interrupted";
		case "unresolved":
			return "unresolved";
		case "failed":
			return "failed";
	}
	switch (status) {
		case "pending":
		case "in_progress":
			return "running";
		case "failed":
			return "failed";
		default:
			return "completed";
	}
}

export function exitCode(result: unknown): number | undefined {
	if (typeof result !== "object" || result === null) return undefined;
	if ("output" in result) {
		const code = codeIn(result.output);
		if (code !== undefined) return code;
	}
	return codeIn(result);
}

function codeIn(value: unknown): number | undefined {
	if (typeof value !== "object" || value === null) return undefined;
	if ("exit_code" in value && typeof value.exit_code === "number") return value.exit_code;
	if ("exitCode" in value && typeof value.exitCode === "number") return value.exitCode;
	return undefined;
}

export type ToolOutput = { format: "text" | "json"; text: string };

// A result is `{ content, output }`: the ACP content blocks, then whatever the harness reported raw.
export function toolOutput(result: unknown): ToolOutput | undefined {
	if (typeof result !== "object" || result === null) return undefined;
	const blocks = "content" in result && Array.isArray(result.content) ? result.content : [];
	const texts = blocks.flatMap((block: unknown) => {
		const text = blockText(block);
		return text === undefined ? [] : [text];
	});
	if (texts.length > 0) return { format: "text", text: texts.join("\n") };
	if (blocks.length > 0) return { format: "json", text: JSON.stringify(blocks, null, 2) };

	const raw = "output" in result ? result.output : undefined;
	if (typeof raw === "string") return raw === "" ? undefined : { format: "text", text: raw };
	if (typeof raw !== "object" || raw === null) return undefined;
	const streams = [field(raw, "stdout"), field(raw, "stderr")].filter(
		(stream) => stream !== undefined && stream !== "",
	);
	if (streams.length > 0) return { format: "text", text: streams.join("") };
	const rest = Object.entries(raw).filter(
		([key]) => !["exit_code", "exitCode", "stdout", "stderr"].includes(key),
	);
	return rest.length === 0
		? undefined
		: { format: "json", text: JSON.stringify(Object.fromEntries(rest), null, 2) };
}

function blockText(block: unknown): string | undefined {
	if (typeof block !== "object" || block === null || !("content" in block)) return undefined;
	const content = block.content;
	if (typeof content !== "object" || content === null) return undefined;
	return field(content, "text");
}

function field(value: object, name: string): string | undefined {
	const found: unknown = Object.entries(value).find(([key]) => key === name)?.[1];
	return typeof found === "string" ? found : undefined;
}

export function payloadReference(value: unknown): PayloadReference | undefined {
	if (typeof value !== "object" || value === null) return undefined;
	if (!("payload_id" in value) || typeof value.payload_id !== "string") return undefined;
	if (!("bytes" in value) || typeof value.bytes !== "number") return undefined;
	const media =
		"media_type" in value && value.media_type === "application/json"
			? "application/json"
			: "text/plain; charset=utf-8";
	return { payload_id: value.payload_id, bytes: value.bytes, media_type: media };
}

export function planStep(value: unknown): PlanStep | undefined {
	if (typeof value !== "object" || value === null) return undefined;
	if (!("content" in value) || typeof value.content !== "string") return undefined;
	return {
		content: value.content,
		priority: "priority" in value && typeof value.priority === "string" ? value.priority : "medium",
		status: "status" in value && typeof value.status === "string" ? value.status : "pending",
	};
}

export function entryText(entry: Entry): string {
	switch (entry.type) {
		case "said":
			return typeof entry.message === "string"
				? `${entry.participant}: ${entry.message}`
				: `${entry.participant}:`;
		case "brief":
			return typeof entry.brief === "string" ? entry.brief : "Brief";
		case "participant_joined":
			return `${entry.participant} joined`;
		case "session_started":
			return `${entry.agent} started`;
		case "session_ended":
			return exitLine(entry.exit);
		case "turn_interrupted":
			return `Interrupted by ${entry.participant}`;
		case "option_changed":
			return entry.refused
				? `Option ${entry.option} refused: ${entry.refused}`
				: `Option ${entry.option}: ${entry.from ?? "default"} → ${entry.to ?? "default"}`;
		case "thought":
			return typeof entry.text === "string" ? entry.text : "Thought";
		case "plan":
			return Array.isArray(entry.entries) ? `Plan: ${entry.entries.length} steps` : "Plan";
		case "messages":
			return Array.isArray(entry.messages)
				? entry.messages
						.map((message) =>
							typeof message.message === "string"
								? `${message.participant}: ${message.message}`
								: `${message.participant}:`,
						)
						.join("\n")
				: "Messages";
		case "pull_request":
			return `Pull request ${entry.number}: ${entry.title}`;
		case "tool_call":
			return entry.title;
		case "instance_released":
			return "Instance released";
		case "expired":
			return "expired";
		default: {
			const kind = (entry as { type?: unknown }).type;
			return typeof kind === "string" ? kind : "entry";
		}
	}
}

export function firstLine(text: string): string {
	const line = text.split("\n")[0] ?? text;
	return line.length > 160 ? `${line.slice(0, 159)}…` : line;
}

export function thoughtLine(text: string): string {
	return firstLine(text).replace(/[`*]/g, "");
}

export type Spoken = {
	key: string;
	participant: string;
	speaker: "agent" | "person";
	body: string | PayloadReference;
};

// Only a Session's own report carries its id, so that is what marks the Agent among Participants.
export function spoken({ seq, entry }: Delivered): Spoken[] {
	switch (entry.type) {
		case "said":
			return [
				{
					key: `${seq}:0`,
					participant: entry.participant,
					speaker: entry.session_id ? "agent" : "person",
					body: entry.message,
				},
			];
		case "messages":
			return Array.isArray(entry.messages)
				? entry.messages.map((message, index) => ({
						key: `${seq}:${index}`,
						participant: message.participant,
						speaker: "person",
						body: message.message,
					}))
				: [];
		default:
			return [];
	}
}

export function exitLine(exit: Exit): string {
	if (exit.status === "succeeded") return "Session succeeded";
	return exit.because ? `Session failed: ${exit.because}` : "Session failed";
}
