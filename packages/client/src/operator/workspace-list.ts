import { ago, reasonsText } from "./format";
import type { PullRequest, Workspace, WorkspaceListed, WorkspaceWork } from "./generated";
import { phaseOf, type RowPhase } from "./session-state";

const RANK: Record<RowPhase, number> = {
	attention: 0,
	working: 1,
	waiting: 2,
	queued: 3,
	idle: 4,
};

export function order(rows: WorkspaceListed[]): WorkspaceListed[] {
	return rows.toSorted((one, other) => {
		const rank = RANK[phaseOf(one)] - RANK[phaseOf(other)];
		if (rank !== 0) return rank;
		if (phaseOf(one) === "queued") {
			const position =
				(one.queue?.position ?? Number.MAX_SAFE_INTEGER) -
				(other.queue?.position ?? Number.MAX_SAFE_INTEGER);
			if (position !== 0) return position;
		}
		return enqueued(one) - enqueued(other) || one.name.localeCompare(other.name);
	});
}

function enqueued(row: WorkspaceListed): number {
	const at = row.session?.enqueued_at ?? row.opened_at;
	return Date.parse(at) || 0;
}

export function waitingText(row: WorkspaceListed): string | undefined {
	const reasons = row.queue?.reasons ?? [];
	if (reasons.length > 0) return reasonsText(reasons);
	if (row.queue?.pending_since) return `input held since ${ago(row.queue.pending_since)}`;
	return undefined;
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
			return `reported ${ago(work.reported_at)}`;
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
