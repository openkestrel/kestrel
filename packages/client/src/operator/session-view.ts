import type {
	ChangingOption,
	Presence,
	RunningTool,
	RunningUnit,
	Session,
	SessionCommand,
	SessionObservation,
	SessionOption,
	TranscriptSessionState,
	Usage,
	Workspace,
} from "./generated";

export function sessionTitle(session: Session | undefined, workspace: Workspace): string {
	if (session?.title) return session.title;
	if (session) return session.name;
	return workspace.name;
}

export function modelLine(session: Session | undefined): string {
	const requested = session?.model ?? "the harness's default";
	const running = session?.worked_model;
	if (running && running !== session.model) return `requested ${requested} · running ${running}`;
	return `model ${running ?? requested}`;
}

export function continuityLine(
	workspace: Workspace,
	workspaces: Workspace[] | undefined,
): string | undefined {
	const named = new Map((workspaces ?? []).map((known) => [known.id, known.name]));
	const parts: string[] = [];
	if (workspace.continues) {
		parts.push(`continues ${named.get(workspace.continues) ?? workspace.continues}`);
	}
	if (workspace.continued_by.length > 0) {
		parts.push(
			`continued by ${workspace.continued_by.map((id) => named.get(id) ?? id).join(", ")}`,
		);
	}
	return parts.length > 0 ? parts.join(" · ") : undefined;
}

export function usageLine(usage: Usage | null | undefined): string | undefined {
	if (!usage) return undefined;
	const tokens = `${usage.context_used.toLocaleString()} of ${usage.context_size.toLocaleString()} tokens`;
	return usage.cost ? `${tokens} · ${usage.cost.amount.toFixed(2)} ${usage.cost.currency}` : tokens;
}

export function followersLine(presence: Presence | undefined): string | undefined {
	if (!presence) return undefined;
	const anonymous = presence.anonymous === 0 ? undefined : `${presence.anonymous} anonymous`;
	const watching = [presence.named.join(", "), anonymous].filter(Boolean).join(" · ");
	return watching ? `watching ${watching}` : "no one else is watching";
}

export function commandLine(command: SessionCommand): string {
	return command.input_hint ?? command.name;
}

export function optionCurrent(option: SessionOption): string {
	if (typeof option.current === "boolean") return option.current ? "on" : "off";
	return option.current;
}

export function pendingLine(change: ChangingOption): string {
	return `${change.participant} is changing ${change.option} to ${change.value}`;
}

export function interruptingLabel(session: Session | undefined): string | undefined {
	if (!session?.interrupting) return undefined;
	return `${session.interrupting.participant} asked this Turn to stop`;
}

export function mayInterrupt(session: Session | undefined): boolean {
	return session?.state === "working";
}

export type OpenWork = {
	key: string;
	title: string;
	kind: "tool" | "subagent" | "background task";
};

export type Trailing =
	| { observed: "open"; work: OpenWork[] }
	| { observed: "settling" }
	| { observed: "unavailable"; last: { observedAt: string; work: OpenWork[] } | null };

type Observed = {
	observation: SessionObservation;
	tools: RunningTool[];
	units: RunningUnit[];
};

// The follow's snapshot is newer than the Session read, but only when it is about this Session.
export function trailingOf(
	session: Session | undefined,
	live: TranscriptSessionState | undefined,
): Trailing | undefined {
	if (session?.state !== "trailing") return undefined;
	const seen: Observed = live?.session_id === session.id ? live : session;
	if (seen.observation.availability === "unavailable") {
		const { last } = seen.observation;
		return {
			observed: "unavailable",
			last: last && { observedAt: last.observed_at, work: openWork(last.tools, last.units) },
		};
	}
	const work = openWork(seen.tools, seen.units);
	return work.length > 0 ? { observed: "open", work } : { observed: "settling" };
}

function openWork(tools: RunningTool[], units: RunningUnit[]): OpenWork[] {
	return [
		...tools.map((tool): OpenWork => ({
			key: `tool:${tool.call_id}`,
			title: tool.title,
			kind: "tool",
		})),
		...units.map((unit): OpenWork => ({
			key: `unit:${unit.id}`,
			title: unit.title,
			kind: unit.kind === "subagent" ? "subagent" : "background task",
		})),
	];
}
