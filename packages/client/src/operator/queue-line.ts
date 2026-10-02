import { reasonText } from "./format";
import type { Queue } from "./generated";

export function openingQueueLine(queue: Queue | undefined, briefed: boolean): string {
	if (!queue?.work_role) {
		return "No work role is dispatching, so the Session would wait.";
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
	if (queued) {
		if (queued.position !== null) {
			return `Queued at position ${queued.position}.`;
		}
		return `Waiting: ${queued.reasons.map(reasonText).join("; ")}.`;
	}

	const waiting = queue.waiting.find((row) => row.workspace === workspace);
	if (waiting && waiting.reasons.length > 0) {
		return `Waiting: ${waiting.reasons.map(reasonText).join("; ")}.`;
	}

	return undefined;
}
