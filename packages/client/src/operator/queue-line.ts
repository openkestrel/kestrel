import { reasonText } from "./format";
import type { Queue } from "./generated";

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
	const queued = queue.queued.find((row) => row.workspace === workspace);
	const waiting = queue.waiting.find((row) => row.workspace === workspace);
	const unbriefed = queue.unbriefed.find((row) => row.workspace === workspace);
	const row = queued ?? waiting ?? unbriefed;
	if (!row) return undefined;
	const label = queued ? "Queued" : waiting ? "Next Turn" : "First Turn";
	const position = row.position !== null ? `${label} at position ${row.position}.` : undefined;
	const reasons =
		row.reasons.length > 0 ? `Waiting: ${row.reasons.map(reasonText).join("; ")}.` : undefined;
	if (position) return reasons ? `${position} ${reasons}` : position;
	if (!queue.work_role) {
		const unknown = "Queue order is unknown: no dispatch configuration is recorded.";
		return reasons ? `${reasons} ${unknown}` : unknown;
	}
	return reasons;
}
