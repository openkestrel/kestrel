import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Refusal } from "#/components/refusal";
import { Badge } from "#/components/ui/badge";
import { Button } from "#/components/ui/button";
import { cacheWarning, mayWriteOptions, optionChange, optionValues } from "#/operator/composer";
import type {
	Presence,
	Session,
	SessionOption,
	TranscriptSessionState,
	Workspace,
} from "#/operator/generated";
import { useParticipant } from "#/operator/participant";
import { operator, sessionsKey } from "#/operator/queries";
import {
	commandLine,
	continuityLine,
	followersLine,
	interruptingLabel,
	modelLine,
	optionCurrent,
	pendingLine,
	sessionTitle,
	stateLabel,
	usageLine,
} from "#/operator/session-view";
import { changeSessionOption } from "#/operator/turns";

export function SessionHeader({
	organization,
	read,
	session,
	live,
	presence,
	workspaces,
}: {
	organization: string;
	read: Workspace;
	session: Session | undefined;
	live: TranscriptSessionState | undefined;
	presence: Presence | undefined;
	workspaces: Workspace[] | undefined;
}) {
	const name = useParticipant();
	const queryClient = useQueryClient();
	const [refusal, setRefusal] = useState<unknown>(null);
	const [confirming, setConfirming] = useState<{ option: SessionOption; value: string } | null>(
		null,
	);
	const [writing, setWriting] = useState(false);

	const usage = live?.usage ?? session?.usage ?? undefined;
	const usageText = usageLine(usage);
	const followers = followersLine(presence);
	const continuity = continuityLine(read, workspaces);
	const interrupting = interruptingLabel(session);
	const options = session?.options ?? [];
	const canWrite = mayWriteOptions(session?.state) && name !== null;

	async function write(option: SessionOption, value: string) {
		if (!session || !name) return;
		setWriting(true);
		setRefusal(null);
		try {
			await changeSessionOption(
				operator,
				organization,
				session.id,
				optionChange(option, value, name),
			);
			setConfirming(null);
			await queryClient.invalidateQueries({ queryKey: sessionsKey(organization) });
		} catch (error) {
			setRefusal(error);
		} finally {
			setWriting(false);
		}
	}

	function choose(option: SessionOption, value: string) {
		if (cacheWarning(option, usage)) {
			setConfirming({ option, value });
			return;
		}
		void write(option, value);
	}

	return (
		<header className="shrink-0 border-b px-4 py-2" data-session-header>
			<div className="flex flex-wrap items-center gap-2">
				<p className="truncate font-semibold text-sm" data-session-title>
					{sessionTitle(session, read)}
				</p>
				<Badge variant="secondary" data-session-state>
					{stateLabel(session)}
				</Badge>
				{interrupting && (
					<span className="text-muted-foreground text-xs" data-session-interrupting>
						{interrupting}
					</span>
				)}
			</div>
			<dl className="mt-1 grid gap-x-4 gap-y-0.5 text-muted-foreground text-xs sm:grid-cols-2">
				<div className="flex gap-1">
					<dt className="sr-only">Model</dt>
					<dd data-session-model>{modelLine(session)}</dd>
				</div>
				{continuity && (
					<div className="flex gap-1">
						<dt className="sr-only">Continuity</dt>
						<dd data-session-continuity>{continuity}</dd>
					</div>
				)}
				{usageText && (
					<div className="flex gap-1">
						<dt className="sr-only">Usage</dt>
						<dd data-session-usage>{usageText}</dd>
					</div>
				)}
				{followers && (
					<div className="flex gap-1">
						<dt className="sr-only">Followers</dt>
						<dd data-session-followers>{followers}</dd>
					</div>
				)}
				{session && session.commands.length > 0 && (
					<div className="flex gap-1">
						<dt className="sr-only">Commands</dt>
						<dd data-session-commands>{session.commands.map(commandLine).join(" · ")}</dd>
					</div>
				)}
			</dl>
			{options.length > 0 && (
				<div className="mt-2 grid gap-1" data-session-options>
					{options.map((option) => (
						<div
							key={option.id}
							className="flex flex-wrap items-center gap-1 text-xs"
							data-option={option.id}
						>
							<span className="text-muted-foreground">{option.name}:</span>
							<span data-option-current>{optionCurrent(option)}</span>
							{optionValues(option)
								.filter((value) => value.value !== String(option.current))
								.map((value) => (
									<Button
										key={value.value}
										data-option-value={value.value}
										disabled={!canWrite || writing}
										onClick={() => choose(option, value.value)}
										size="xs"
										type="button"
										variant="outline"
									>
										{value.name}
									</Button>
								))}
							{option.kind === "boolean" && (
								<Button
									data-option-toggle
									disabled={!canWrite || writing}
									onClick={() => choose(option, option.current ? "false" : "true")}
									size="xs"
									type="button"
									variant="outline"
								>
									{option.current ? "turn off" : "turn on"}
								</Button>
							)}
						</div>
					))}
					{session?.changing_options.map((change) => (
						<p
							key={`${change.option}:${change.participant}`}
							className="text-muted-foreground text-xs"
							data-changing-option
						>
							{pendingLine(change)}
						</p>
					))}
					{!name && (
						<p className="text-muted-foreground text-xs" data-option-note>
							Declare your name in the composer to change options
						</p>
					)}
				</div>
			)}
			{confirming && (
				<div className="mt-2 border border-border p-2 text-xs" data-option-confirm>
					<p>{cacheWarning(confirming.option, usage)}</p>
					<p className="text-muted-foreground">
						Change {confirming.option.name} to {confirming.value}?
					</p>
					<div className="mt-1 flex gap-1">
						<Button
							disabled={writing}
							onClick={() => void write(confirming.option, confirming.value)}
							size="xs"
							type="button"
						>
							Change
						</Button>
						<Button onClick={() => setConfirming(null)} size="xs" type="button" variant="ghost">
							Cancel
						</Button>
					</div>
				</div>
			)}
			{refusal !== null && (
				<div className="mt-2">
					<Refusal error={refusal} />
				</div>
			)}
		</header>
	);
}
