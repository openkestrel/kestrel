import { useState } from "react";
import {
	Conversation,
	ConversationContent,
	ConversationEmptyState,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import { ToggleGroup, ToggleGroupItem } from "#/components/ui/toggle-group";
import { useTranscript } from "#/operator/currency";
import type { Currency } from "#/operator/currency";
import type { Session, Workspace } from "#/operator/generated";
import { flow } from "#/operator/transcript-view";
import { BriefComposer } from "./brief-composer";
import { Composer } from "./composer";
import { SessionHeader } from "./session-header";
import { ActivityRow } from "./transcript/activity-row";
import { EntryRow, type Disclosure } from "./transcript/entry-row";
import { LiveLine } from "./transcript/live-line";

const MODES: { value: Disclosure; label: string }[] = [
	{ value: "line", label: "One line" },
	{ value: "steps", label: "Steps" },
	{ value: "full", label: "Full" },
];

export function TranscriptPane({
	currency,
	organization,
	workspace,
	read,
	session,
	workspaces,
}: {
	currency: Currency;
	organization: string;
	workspace: string;
	read: Workspace;
	session: Session | undefined;
	workspaces: Workspace[] | undefined;
}) {
	const transcript = useTranscript(currency, organization, workspace);
	const [mode, setMode] = useState<Disclosure>("line");
	const [overrides, setOverrides] = useState<ReadonlyMap<number, boolean>>(new Map());

	const items = flow(transcript.entries, transcript.activities);
	const emptyOfEverything = items.length === 0 && transcript.sessionState === undefined;
	const hasBrief = transcript.entries.some(({ entry }) => entry.type === "brief");
	const settling =
		session === undefined || session.state === "queued" || session.state === "unbriefed";
	// A mirror resumed from a stored cursor means this browser has followed the Workspace before,
	// so the turn composer owns it and the Brief composer stays out of the way.
	const briefing = read.state === "open" && !hasBrief && settling && !transcript.resumed;

	return (
		<>
			<SessionHeader
				live={transcript.sessionState}
				organization={organization}
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
			<Conversation>
				<ConversationContent className="gap-2">
					{emptyOfEverything ? (
						<ConversationEmptyState
							title="Transcript"
							description={`${read.project} on ${read.checkout.branch}`}
						/>
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
			{read.state === "open" && (
				<>
					<div hidden={!briefing}>
						<BriefComposer briefed={!briefing} organization={organization} record={read} />
					</div>
					{!briefing && (
						<Composer
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
