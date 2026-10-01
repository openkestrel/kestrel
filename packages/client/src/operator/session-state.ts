import type { Session, Workspace } from "./generated";

export type RowPhase = "attention" | "working" | "waiting" | "queued" | "idle";

export type SessionRow = {
	workspace: Pick<Workspace, "held">;
	session: Session | undefined;
	position: number | null;
};

// A row needs a person: its Instance is held for work that exists nowhere else (GLOSSARY), its
// Session lost its supervisor, or its Session failed.
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

export function preparingLabel(preparing: Session["preparing"] | undefined): string {
	switch (preparing) {
		case "provisioning":
			return "provisioning";
		case "cloning":
			return "cloning";
		case "harness_ready":
			return "harness ready";
		default:
			return "preparing";
	}
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
			return `Preparing (${preparingLabel(session.preparing)})`;
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
		case "cloning":
		case "harness_ready":
			return preparingLabel(session.preparing);
		default:
			break;
	}
	if (session.thought_buffering) return "thinking";
	if (session.message_buffering) return "writing";
	return undefined;
}
