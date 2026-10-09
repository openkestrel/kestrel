import { reasonsText } from "./format";
import type { Queue } from "./generated";

export type QueueRead =
	| { kind: "reading"; delayed: boolean }
	| { kind: "failed"; error: unknown; known: Queue | undefined; retrying: boolean }
	| { kind: "read"; queue: Queue; empty: boolean };

export type QueueQueryState = { data: Queue | undefined; error: unknown; fetching: boolean };

export function queueRead({ data, error, fetching }: QueueQueryState, delayed: boolean): QueueRead {
	if (error !== null && error !== undefined) {
		return { kind: "failed", error, known: data, retrying: fetching };
	}
	if (data === undefined) return { kind: "reading", delayed };
	return { kind: "read", queue: data, empty: queuedSessions(data).length === 0 };
}

export type QueuedSession = {
	name: string;
	workspace: string;
	label: "Queued" | "Next Turn" | "First Turn";
	position: number | null;
	reasons: string;
};

export function queuedSessions(queue: Queue): QueuedSession[] {
	return [
		...queue.queued.map((row) => ({ row, label: "Queued" as const })),
		...queue.waiting.map((row) => ({ row, label: "Next Turn" as const })),
		...queue.unbriefed.map((row) => ({ row, label: "First Turn" as const })),
	].map(({ row, label }) => ({
		name: row.name,
		workspace: row.workspace,
		label,
		position: row.position,
		reasons: reasonsText(row.reasons),
	}));
}
