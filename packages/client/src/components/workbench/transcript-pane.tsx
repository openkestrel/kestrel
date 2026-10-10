import { useEffect, useRef, useState } from "react";
import {
	Conversation,
	ConversationContent,
	ConversationEmptyState,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import { ToggleGroup, ToggleGroupItem } from "#/components/ui/toggle-group";
import type { Session, Workspace } from "#/operator/generated";
import { sessionPhase } from "#/operator/session-state";
import type { TranscriptSnapshot } from "#/operator/transcript";
import { entryText, flow } from "#/operator/transcript-view";
import { BriefComposer } from "./brief-composer";
import { Composer } from "./composer";
import { useOptionWrite } from "./option-write";
import { SessionHeader } from "./session-header";
import { ActivityRow } from "./transcript/activity-row";
import { ConnectionNotice } from "./transcript/connection-notice";
import { EntryRow, type Disclosure } from "./transcript/entry-row";
import { LiveLine } from "./transcript/live-line";

const MODES: { value: Disclosure; label: string }[] = [
	{ value: "line", label: "One line" },
	{ value: "steps", label: "Steps" },
	{ value: "full", label: "Full" },
];

export function TranscriptPane({
	transcript,
	reconnect,
	organization,
	workspace,
	read,
	session,
	workspaces,
}: {
	transcript: TranscriptSnapshot;
	reconnect: () => void;
	organization: string;
	workspace: string;
	read: Workspace;
	session: Session | undefined;
	workspaces: Workspace[] | undefined;
}) {
	const optionWrite = useOptionWrite(
		organization,
		session,
		transcript.sessionState?.usage ?? session?.usage,
	);
	const [mode, setMode] = useState<Disclosure>("line");
	const [overrides, setOverrides] = useState<ReadonlyMap<number, boolean>>(new Map());
	const [announced, setAnnounced] = useState("");
	const floor = useRef<number | undefined>(undefined);

	// The first pass only sets the floor, so the history a fresh mount replays is not announced.
	useEffect(() => {
		const shared = transcript.entries.filter((entry) => entry.kind === "shared_state");
		const highest = shared.at(-1)?.seq;
		if (highest === undefined) return;
		if (floor.current === undefined) {
			floor.current = highest;
			return;
		}
		const fresh = shared.filter((entry) => entry.seq > (floor.current ?? 0));
		floor.current = highest;
		const last = fresh.at(-1);
		if (last) setAnnounced(entryText(last.entry));
	}, [transcript.entries]);

	const phase = session ? sessionPhase(session) : undefined;
	const phaseFloor = useRef<string | undefined>(undefined);
	useEffect(() => {
		if (phase === undefined) return;
		if (phaseFloor.current === phase) return;
		const first = phaseFloor.current === undefined;
		phaseFloor.current = phase;
		if (!first && session) setAnnounced(phaseLine(session));
	}, [phase, session]);

	const items = flow(transcript.entries, transcript.activities);
	const emptyOfEverything = items.length === 0 && transcript.sessionState === undefined;
	const hasBrief = transcript.entries.some(({ entry }) => entry.type === "brief");
	const settling =
		session === undefined || session.state === "queued" || session.state === "unbriefed";
	const briefing = read.state === "open" && !hasBrief && settling;

	return (
		<>
			<SessionHeader
				live={transcript.sessionState}
				optionWrite={optionWrite}
				presence={transcript.presence}
				read={read}
				session={session}
				workspaces={workspaces}
			/>
			<div className="flex shrink-0 items-center border-b px-2 py-1.5">
				<ToggleGroup
					aria-label="Disclosure"
					size="sm"
					value={[mode]}
					onValueChange={(values) => {
						const next = values.at(-1);
						if (next === "line" || next === "steps" || next === "full") setMode(next);
					}}
				>
					{MODES.map(({ value, label }) => (
						<ToggleGroupItem key={value} value={value}>
							{label}
						</ToggleGroupItem>
					))}
				</ToggleGroup>
			</div>
			<ConnectionNotice connection={transcript.connection} reconnect={reconnect} />
			<Conversation aria-live="off">
				<ConversationContent className="gap-2">
					{emptyOfEverything ? (
						// Reconnecting follows a live read, so an empty history then is known to be empty.
						transcript.connection.state === "live" ||
						transcript.connection.state === "reconnecting" ? (
							<ConversationEmptyState
								title="Transcript"
								description={`${read.project} on ${read.checkout.branch}`}
							/>
						) : transcript.connection.state === "connecting" ? (
							<ConversationEmptyState data-transcript-connection="connecting">
								<output className="text-muted-foreground text-sm">
									Connecting to the Transcript…
								</output>
							</ConversationEmptyState>
						) : null
					) : (
						<>
							{items.map((item) =>
								item.kind === "activity" ? (
									<ActivityRow
										key={`activity-${item.activity.first_seq}`}
										activity={item.activity}
										mode={mode}
										override={overrides.get(item.activity.first_seq)}
										onToggle={(open) =>
											setOverrides((current) => new Map(current).set(item.activity.first_seq, open))
										}
										organization={organization}
										workspace={workspace}
									/>
								) : (
									<EntryRow
										key={`entry-${item.entry.seq}`}
										entry={item.entry}
										mode={mode}
										organization={organization}
										workspace={workspace}
									/>
								),
							)}
							<LiveLine state={transcript.sessionState} />
						</>
					)}
				</ConversationContent>
				<ConversationScrollButton />
			</Conversation>
			<p aria-live="polite" className="sr-only" data-transcript-announcement>
				{announced}
			</p>
			{read.state === "open" && (
				<>
					<div hidden={!briefing}>
						<BriefComposer briefed={!briefing} organization={organization} record={read} />
					</div>
					{!briefing && (
						<Composer
							optionWrite={optionWrite}
							organization={organization}
							read={read}
							session={session}
							workspace={workspace}
						/>
					)}
				</>
			)}
		</>
	);
}

function phaseLine(session: Session): string {
	if (session.state === "ended" && session.exit?.status === "failed") {
		return session.exit.because ? `Session failed: ${session.exit.because}` : "Session failed";
	}
	return `Session ${sessionPhase(session).toLowerCase()}`;
}
