import type { Queue, QueueReason } from "./generated";

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
		return `Waiting: ${queued.reasons.map(reasonInWords).join("; ")}.`;
	}

	const waiting = queue.waiting.find((row) => row.workspace === workspace);
	if (waiting && waiting.reasons.length > 0) {
		return `Waiting: ${waiting.reasons.map(reasonInWords).join("; ")}.`;
	}

	return undefined;
}

export function reasonInWords(reason: QueueReason): string {
	switch (reason.kind) {
		case "dependencies":
			return `waits on ${reason.sessions.join(", ")}`;
		case "subscription_profile":
			return `the Subscription Profile ${reason.profile} is held by ${reason.session}`;
		case "instance_archiving":
			return `waits for the Instance ${reason.instance} to be archived`;
		case "live_instance_limit":
			return `at the limit of ${reason.limit} live Instance${reason.limit === 1 ? "" : "s"}`;
		case "active_work_slots":
			return `all ${reason.limit} Active-Work Slot${reason.limit === 1 ? "" : "s"} occupied`;
		case "ahead":
			return `behind ${reason.sessions.join(", ")}`;
		default: {
			const unknown: never = reason;
			throw new Error(`an unknown queue reason: ${JSON.stringify(unknown)}`);
		}
	}
}
