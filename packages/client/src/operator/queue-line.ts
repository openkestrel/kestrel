import type { Queue } from "./generated";
import { queuedSessions } from "./queue-read";

export function openingQueueLine(queue: Queue | undefined, briefed: boolean): string {
	if (!queue?.work_role) {
		return "No dispatch configuration is recorded, so queue order is unknown.";
	}
	if (!briefed) {
		return "Without a Brief, the Session starts preparing at once and waits for its first message.";
	}

	const { limit, occupied } = queue.active_work;
	if (limit === null) {
		return "The Session would start now.";
	}
	if (occupied < limit) {
		return `The Session would start now: ${occupied} of ${limit} Active-Work Slots occupied.`;
	}

	return `The Session would wait: all ${limit} Active-Work Slots are occupied.`;
}

export function sessionQueueLine(queue: Queue, workspace: string): string | undefined {
	const row = queuedSessions(queue).find((queued) => queued.workspace === workspace);
	if (!row) return undefined;
	const position = row.position !== null ? `${row.label} at position ${row.position}.` : undefined;
	const reasons =
		row.reasons === ""
			? undefined
			: `${row.reasons.charAt(0).toUpperCase()}${row.reasons.slice(1)}.`;
	if (position) return reasons ? `${position} ${reasons}` : position;
	if (!queue.work_role) {
		const unknown = "Queue order is unknown: no dispatch configuration is recorded.";
		return reasons ? `${reasons} ${unknown}` : unknown;
	}
	return reasons;
}
