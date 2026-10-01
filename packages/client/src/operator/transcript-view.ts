import type { Activity, Entry, PayloadReference } from "./generated";
import type { Delivered } from "./transcript";

export type FlowItem =
	| { kind: "entry"; entry: Delivered }
	| { kind: "activity"; activity: Activity };

// Activities and shared-state entries in one order: an Activity's summary sits where its first
// omitted seq does, ahead of the entry that closed it.
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

export function activityWindow(activity: Activity): string {
	return `${activity.first_seq}–${activity.last_seq}`;
}

export function activityCounts(activity: Activity): string {
	const counts = activity.counts;
	const parts: string[] = [];
	if (counts.tool_calls > 0)
		parts.push(`${counts.tool_calls} ${counts.tool_calls === 1 ? "tool" : "tools"}`);
	if (counts.failed_calls > 0) parts.push(`${counts.failed_calls} failed`);
	if (counts.thoughts > 0)
		parts.push(`${counts.thoughts} ${counts.thoughts === 1 ? "thought" : "thoughts"}`);
	if (counts.plans > 0) parts.push(`${counts.plans} ${counts.plans === 1 ? "plan" : "plans"}`);
	if (counts.tombstones > 0) parts.push(`${counts.tombstones} expired`);
	return parts.join(", ");
}

export function activityDuration(activity: Activity): string | undefined {
	if (!activity.started_at || !activity.finished_at) return undefined;
	return elapsed(Date.parse(activity.finished_at) - Date.parse(activity.started_at));
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

export function toolState(status: string): ToolState {
	switch (status) {
		case "pending":
		case "in_progress":
			return "running";
		case "failed":
			return "failed";
		case "interrupted":
			return "interrupted";
		case "unresolved":
			return "unresolved";
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

export type PlanStep = { content: string; priority: string; status: string };

// The plan entry schema in openapi/operator.json refers to itself, so a step is read here rather
// than typed from the generated Entry.
export function planStep(value: unknown): PlanStep | undefined {
	if (typeof value !== "object" || value === null) return undefined;
	if (!("content" in value) || typeof value.content !== "string") return undefined;
	return {
		content: value.content,
		priority: "priority" in value && typeof value.priority === "string" ? value.priority : "medium",
		status: "status" in value && typeof value.status === "string" ? value.status : "pending",
	};
}

export function bytes(count: number): string {
	if (count < 1024) return `${count} B`;
	if (count < 1024 * 1024) return `${Math.round(count / 1024)} KiB`;
	return `${(count / (1024 * 1024)).toFixed(1)} MiB`;
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
			return "Session ended";
		case "turn_interrupted":
			return `${entry.participant} interrupted the turn`;
		case "option_changed":
			return `${entry.participant} changed ${entry.option}${
				entry.refused ? `: ${entry.refused}` : entry.to ? ` to ${entry.to}` : ""
			}`;
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
			const unhandled: never = entry;
			throw new Error(`no such Transcript entry: ${String(unhandled)}`);
		}
	}
}

export function firstLine(text: string): string {
	const line = text.split("\n")[0] ?? text;
	return line.length > 160 ? `${line.slice(0, 159)}…` : line;
}
