import type { FileEntry, Presence, Session } from "./generated";
import type { Delivered } from "./transcript";
import { when } from "./workspace-list";

export function shortRevision(revision: string): string {
	return revision.slice(0, 8);
}

export function pushedText(pushed: string | null): string {
	return pushed === null ? "nothing pushed" : `pushed ${shortRevision(pushed)}`;
}

export function changesText(changes: { files: number; added: number; removed: number }): string {
	return `${changes.files} ${changes.files === 1 ? "file" : "files"} +${changes.added} −${changes.removed}`;
}

export function commitsText(commits: { commits: number; added: number; removed: number }): string {
	return `${commits.commits} ${commits.commits === 1 ? "commit" : "commits"} +${commits.added} −${commits.removed}`;
}

export function untrackedText(count: number): string {
	return `${count} ${count === 1 ? "file" : "files"}`;
}

export function stashedText(count: number): string {
	return `${count} ${count === 1 ? "stash" : "stashes"}`;
}

export function repositoryName(repository: { repository: string }): string {
	const parts = repository.repository.split("/");
	return parts.at(-1) ?? repository.repository;
}

export const DIFF_SCOPES = ["unpublished", "changed", "staged"] as const;

export function scopeLabel(scope: string): string {
	switch (scope) {
		case "unpublished":
			return "Unpublished";
		case "changed":
			return "Changed";
		case "staged":
			return "Staged";
		default:
			return scope.startsWith("commit:") ? `Commit ${shortRevision(scope.slice(7))}` : scope;
	}
}

export function parentPath(path: string): string {
	const cut = path.lastIndexOf("/");
	return cut === -1 ? "" : path.slice(0, cut);
}

export function childPath(parent: string, name: string): string {
	return parent === "" ? name : `${parent}/${name}`;
}

export function breadcrumbs(path: string): { label: string; path: string }[] {
	const parts = path === "" ? [] : path.split("/");
	const crumbs = parts.map((part, index) => ({
		label: part,
		path: parts.slice(0, index + 1).join("/"),
	}));
	return [{ label: "repositories", path: "" }, ...crumbs];
}

export function fileSize(bytes: number | undefined): string {
	if (bytes === undefined) return "";
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KiB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

export function fileKindLabel(entry: FileEntry): string {
	if (entry.kind === "directory") return "directory";
	if (entry.kind === "symlink") return "symlink";
	if (entry.kind === "file") return entry.git === "untracked" ? "untracked" : (entry.git ?? "file");
	return "other";
}

export function sessionPhase(session: Session): string {
	switch (session.state) {
		case "queued":
			return "Queued";
		case "working":
			return "Working";
		case "waiting":
			return "Waiting";
		case "unbriefed":
			return `Preparing (${session.preparing ?? "preparing"})`;
		case "ended":
			return "Ended";
		case "unreachable":
			return "Unreachable";
		default: {
			const unhandled: never = session.state;
			throw new Error(`no such Session state: ${String(unhandled)}`);
		}
	}
}

export function sessionOutcome(session: Session): string | undefined {
	if (session.state !== "ended" && session.state !== "unreachable") return undefined;
	if (session.exit) {
		return session.exit.because
			? `${session.exit.status}: ${session.exit.because}`
			: session.exit.status;
	}
	return session.outcome_message ?? session.state;
}

export function sessionContinuity(session: Session): string {
	const parts: string[] = [];
	parts.push(session.instance === null ? "no Instance" : `instance ${session.instance}`);
	if (session.supervisor !== null) parts.push(`supervisor ${session.supervisor}`);
	if (session.connected_at !== null) parts.push(`connected ${when(session.connected_at)}`);
	if (session.lease_expires_at !== null) parts.push(`lease ${when(session.lease_expires_at)}`);
	return parts.join(" · ");
}

export function optionSummary(session: Session): string[] {
	return session.options
		.filter(
			(option) =>
				option.category === "mode" ||
				option.category === "model" ||
				option.category === "thought_level",
		)
		.map((option) => `${option.name}: ${String(option.current)}`);
}

export function usageText(session: Session): string | undefined {
	if (!session.usage) return undefined;
	return `${compact(session.usage.context_used)}/${compact(session.usage.context_size)} context`;
}

function compact(count: number): string {
	if (count < 1000) return String(count);
	if (count < 1_000_000) return `${(count / 1000).toFixed(1)}k`;
	return `${(count / 1_000_000).toFixed(1)}M`;
}

export function joinedParticipants(entries: Delivered[]): string[] {
	const joined: string[] = [];
	for (const entry of entries) {
		if (entry.entry.type === "participant_joined" && !joined.includes(entry.entry.participant)) {
			joined.push(entry.entry.participant);
		}
	}
	return joined;
}

export function presenceOf(
	presence: Presence | undefined,
): { named: string[]; anonymous: number } | undefined {
	return presence;
}
