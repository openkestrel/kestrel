import type {
	PullRequest,
	Queue,
	QueueReason,
	Session,
	Workspace,
	WorkspaceWork,
} from "./generated";

export type RowPhase = "attention" | "working" | "waiting" | "queued" | "idle";

export type WorkspaceRow = {
	workspace: Workspace;
	session: Session | undefined;
	position: number | null;
	reasons: QueueReason[];
	pendingSince: string | null;
};

const RANK: Record<RowPhase, number> = {
	attention: 0,
	working: 1,
	waiting: 2,
	queued: 3,
	idle: 4,
};

export function composeRow(
	workspace: Workspace,
	sessions: Session[] | undefined,
	queue: Queue | undefined,
): WorkspaceRow {
	const session = latestSession(sessions);
	const entry = queueEntry(queue, session, workspace);

	return {
		workspace,
		session,
		position: entry.position,
		reasons: entry.reasons,
		pendingSince: entry.pendingSince,
	};
}

function latestSession(sessions: Session[] | undefined): Session | undefined {
	return sessions?.at(-1);
}

function queueEntry(
	queue: Queue | undefined,
	session: Session | undefined,
	workspace: Workspace,
): { position: number | null; reasons: QueueReason[]; pendingSince: string | null } {
	if (!queue) return { position: null, reasons: [], pendingSince: null };

	const names = session?.name;
	const queued = queue.queued.find(
		(entry) => entry.name === names || entry.workspace === workspace.id,
	);
	if (queued) return { position: queued.position, reasons: queued.reasons, pendingSince: null };

	const waiting = queue.waiting.find(
		(entry) => entry.name === names || entry.workspace === workspace.id,
	);
	if (waiting) {
		return { position: null, reasons: waiting.reasons, pendingSince: waiting.pending_since };
	}

	const unbriefed = queue.unbriefed.find(
		(entry) => entry.name === names || entry.workspace === workspace.id,
	);
	return {
		position: null,
		reasons: [],
		pendingSince: unbriefed?.pending_since ?? null,
	};
}

// A row needs a person: its Instance is held for work that exists nowhere else (GLOSSARY), its
// Session lost its supervisor, or its Session failed.
export function needsAttention(row: WorkspaceRow): boolean {
	if (row.workspace.held !== null) return true;
	if (row.session?.state === "unreachable") return true;
	return row.session?.state === "ended" && row.session.exit?.status === "failed";
}

export function phaseOf(row: WorkspaceRow): RowPhase {
	if (needsAttention(row)) return "attention";
	switch (row.session?.state) {
		case "working":
		case "unbriefed":
			return "working";
		case "waiting":
			return "waiting";
		case "queued":
			return "queued";
		default:
			return "idle";
	}
}

export function order(rows: WorkspaceRow[]): WorkspaceRow[] {
	return rows.toSorted((one, other) => {
		const rank = RANK[phaseOf(one)] - RANK[phaseOf(other)];
		if (rank !== 0) return rank;
		if (phaseOf(one) === "queued") {
			const position =
				(one.position ?? Number.MAX_SAFE_INTEGER) - (other.position ?? Number.MAX_SAFE_INTEGER);
			if (position !== 0) return position;
		}
		return (
			enqueued(one) - enqueued(other) || one.workspace.name.localeCompare(other.workspace.name)
		);
	});
}

function enqueued(row: WorkspaceRow): number {
	const at = row.session?.enqueued_at ?? row.workspace.opened_at;
	return Date.parse(at) || 0;
}

export function phaseLabel(row: WorkspaceRow): string {
	if (needsAttention(row)) return "Attention";
	switch (row.session?.state) {
		case "working":
			return "Working";
		case "unbriefed":
			return "Preparing";
		case "waiting":
			return "Waiting";
		case "queued":
			return row.position === null ? "Queued" : `Queued #${row.position}`;
		case "ended":
			return "Ended";
		case "unreachable":
			return "Unreachable";
		default:
			return "Open";
	}
}

// The line a person scans to see what is happening now: a running tool, the preparing step of an
// unbriefed Session, or what it is writing.
export function currentUnit(session: Session | undefined): string | undefined {
	if (!session) return undefined;
	const running =
		session.tools.find((tool) => tool.status !== "completed" && tool.status !== "failed") ??
		session.tools[0];
	if (running) return running.title;
	switch (session.preparing) {
		case "provisioning":
			return "provisioning";
		case "cloning":
			return "cloning";
		case "harness_ready":
			return "harness ready";
		default:
			break;
	}
	if (session.thought_buffering) return "thinking";
	if (session.message_buffering) return "writing";
	return undefined;
}

export function reasonText(reason: QueueReason): string {
	switch (reason.kind) {
		case "dependencies":
			return `waiting on ${list(reason.sessions)}`;
		case "subscription_profile":
			return `${reason.session} holds the ${reason.profile} profile`;
		case "instance_archiving":
			return `archiving ${reason.instance} to make room`;
		case "live_instance_limit":
			return `at the live Instance limit of ${reason.limit}`;
		case "active_work_slots":
			return `every Active-Work Slot is occupied (${reason.limit})`;
		case "ahead":
			return `behind ${list(reason.sessions)}`;
		default: {
			const unhandled: never = reason;
			throw new Error(`no such queue reason: ${String(unhandled)}`);
		}
	}
}

export function waitingText(row: WorkspaceRow): string | undefined {
	if (row.reasons.length > 0) return row.reasons.map(reasonText).join("; ");
	if (row.pendingSince) return `input held since ${when(row.pendingSince)}`;
	return undefined;
}

function list(names: string[]): string {
	if (names.length === 0) return "nothing";
	if (names.length === 1) return names[0] ?? "nothing";
	return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

export type ChangedWork = {
	repositories: number;
	files: number;
	added: number;
	removed: number;
	untracked: number;
	unreadable: number;
};

export function changedWork(work: WorkspaceWork | undefined): ChangedWork | undefined {
	if (!work || work.state !== "reported") return undefined;
	const changed: ChangedWork = {
		repositories: work.repositories.length,
		files: 0,
		added: 0,
		removed: 0,
		untracked: 0,
		unreadable: 0,
	};
	for (const repository of work.repositories) {
		if (repository.git !== "read") {
			changed.unreadable += 1;
			continue;
		}
		changed.files += repository.changed.files + repository.staged.files;
		changed.added += repository.changed.added + repository.staged.added;
		changed.removed += repository.changed.removed + repository.staged.removed;
		changed.untracked += repository.untracked;
	}
	return changed;
}

export function workNote(work: WorkspaceWork | undefined): string | undefined {
	if (!work) return undefined;
	switch (work.state) {
		case "reported":
			return `reported ${when(work.reported_at)}`;
		case "no_instance":
			return "no Instance";
		case "not_answering":
			return work.message;
		default: {
			const unhandled: never = work;
			throw new Error(`no such work reading: ${String(unhandled)}`);
		}
	}
}

export function learnedPullRequest(workspace: Workspace): PullRequest | undefined {
	const known = workspace.pull_requests
		.flatMap((availability) => availability.known ?? [])
		.toSorted((one, other) => Date.parse(other.updated_at) - Date.parse(one.updated_at));
	return known[0];
}

export function pullRequestsUnavailable(workspace: Workspace): boolean {
	return workspace.pull_requests.every(
		(availability) => availability.availability === "unavailable",
	);
}

export function when(timestamp: string): string {
	const at = Date.parse(timestamp);
	if (Number.isNaN(at)) return "at an unknown time";
	const seconds = Math.max(0, (Date.now() - at) / 1000);
	if (seconds < 60) return "just now";
	if (seconds < 3_600) return `${Math.floor(seconds / 60)}m ago`;
	if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h ago`;
	return `${Math.floor(seconds / 86_400)}d ago`;
}
