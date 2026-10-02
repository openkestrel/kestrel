import { preparingStep } from "./format";
import type { Session, Workspace } from "./generated";

export type RowPhase = "attention" | "working" | "waiting" | "queued" | "idle";

export type SessionRow = {
	workspace: Pick<Workspace, "held">;
	session: Session | undefined;
	position: number | null;
};

export function needsAttention(row: SessionRow): boolean {
	if (row.workspace.held !== null) return true;
	if (row.session?.state === "unreachable") return true;
	return row.session?.state === "ended" && row.session.exit?.status === "failed";
}

export function phaseOf(row: SessionRow): RowPhase {
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

export function phaseLabel(row: SessionRow): string {
	if (needsAttention(row)) return "Attention";
	if (!row.session) return "Open";
	if (row.session.state === "queued" && row.position !== null) return `Queued #${row.position}`;
	return sessionPhase(row.session);
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
			return "Preparing";
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

export function currentUnit(session: Session | undefined): string | undefined {
	if (!session) return undefined;
	const running =
		session.tools.find((tool) => tool.status !== "completed" && tool.status !== "failed") ??
		session.tools[0];
	if (running) return running.title;
	if (session.preparing) return preparingStep(session.preparing);
	if (session.thought_buffering) return "thinking";
	if (session.message_buffering) return "writing";
	return undefined;
}
