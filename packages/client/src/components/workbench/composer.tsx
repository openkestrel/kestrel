import { useQueryClient } from "@tanstack/react-query";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import {
	PromptInput,
	PromptInputBody,
	PromptInputTextarea,
	PromptInputFooter,
	PromptInputHeader,
} from "#/components/ai-elements/prompt-input";
import { Refusal } from "#/components/refusal";
import { Alert, AlertTitle } from "#/components/ui/alert";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { modeStep, offeredCommands, partialReport, postLabel } from "#/operator/composer";
import type { Session, TranscriptSessionState, Workspace } from "#/operator/generated";
import { participant, useParticipant } from "#/operator/participant";
import { operator, sessionsKey, workspaceKey } from "#/operator/queries";
import { mayInterrupt } from "#/operator/session-view";
import { interruptTurn, postTurn } from "#/operator/turns";
import { HeldMessages } from "./held-messages";
import type { OptionWrite } from "./option-write";
import { SessionControls } from "./session-controls";
import { useComposerInput } from "./workbench";

export function Composer({
	organization,
	workspace,
	read,
	session,
	live,
	optionWrite,
}: {
	organization: string;
	workspace: string;
	read: Workspace;
	session: Session | undefined;
	live: TranscriptSessionState | undefined;
	optionWrite: OptionWrite;
}) {
	const name = useParticipant();
	const queryClient = useQueryClient();
	const input = useRef<HTMLTextAreaElement>(null);
	const nameInput = useRef<HTMLInputElement>(null);
	const [draft, setDraft] = useState("");
	const [asking, setAsking] = useState(false);
	useEffect(() => {
		if (asking) nameInput.current?.focus();
	}, [asking]);
	useComposerInput(input, !asking);
	const [draftName, setDraftName] = useState("");
	const [nameProblem, setNameProblem] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [interruptPending, setInterruptPending] = useState(false);
	const [refusal, setRefusal] = useState<unknown>(null);
	const [partial, setPartial] = useState<string | null>(null);
	const [notice, setNotice] = useState("");

	const [commandIndex, setCommandIndex] = useState(0);
	const [commandsDismissed, setCommandsDismissed] = useState(false);
	const matches = commandsDismissed ? [] : offeredCommands(session?.commands ?? [], draft);
	function selectCommand(index: number) {
		const command = matches.at(index);
		if (!command) return;
		setDraft(`/${command.name} `);
		setCommandsDismissed(true);
		input.current?.focus();
	}

	const label = postLabel(session?.state);

	function announce(text: string) {
		setNotice(text);
	}

	async function refresh() {
		await queryClient.invalidateQueries({ queryKey: workspaceKey(organization, workspace) });
		await queryClient.invalidateQueries({ queryKey: sessionsKey(organization) });
	}

	function needsName(): boolean {
		if (name !== null) return false;
		setAsking(true);
		return true;
	}

	function remember() {
		const problem = participant.remember(draftName);
		setNameProblem(problem);
		if (problem) return;
		setAsking(false);
		setDraftName("");
		requestAnimationFrame(() => input.current?.focus());
	}

	async function post(): Promise<boolean> {
		if (name === null || draft.trim().length === 0) return false;
		setBusy(true);
		setRefusal(null);
		setPartial(null);
		try {
			const posted = await postTurn(
				operator,
				organization,
				workspace,
				name,
				(session?.commands ?? []).some(
					(command) =>
						draft.startsWith(`/${command.name}`) &&
						(draft.length === command.name.length + 1 ||
							/\s/.test(draft[command.name.length + 1] ?? "")),
				)
					? draft
					: draft.trim(),
			);
			setDraft("");
			announce(posted.held_message ? "Message held for the next turn" : "Message sent");
			await refresh();
			return true;
		} catch (error) {
			setRefusal(error);
			return false;
		} finally {
			setBusy(false);
		}
	}

	async function sendNow() {
		if (needsName() || name === null || !session) return;
		if (!(await post())) return;
		setBusy(true);
		try {
			setInterruptPending(true);
			await interruptTurn(operator, organization, session.id, name);
			await refresh();
		} catch (error) {
			setRefusal(error);
			setPartial(partialReport(true, false) ?? null);
		} finally {
			setInterruptPending(false);
			setBusy(false);
		}
	}

	async function interrupt() {
		if (needsName() || name === null || !session) return;
		setBusy(true);
		setRefusal(null);
		setPartial(null);
		try {
			setInterruptPending(true);
			await interruptTurn(operator, organization, session.id, name);
			await refresh();
		} catch (error) {
			setRefusal(error);
		} finally {
			setInterruptPending(false);
			setBusy(false);
		}
	}

	function cycleMode() {
		if (optionWrite.inFlight()) return;
		const step = modeStep(session?.options ?? [], session?.state, name);
		if ("say" in step) {
			announce(step.say);
			return;
		}
		const warning = optionWrite.choose({
			...step,
			applied: () => announce(`mode ${step.value}`),
		});
		if (warning) announce(warning);
	}

	function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
		if (event.nativeEvent.isComposing) return;
		if (matches.length > 0) {
			if (event.key === "ArrowDown" || event.key === "ArrowUp") {
				event.preventDefault();
				setCommandIndex(
					(index) =>
						(index + (event.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length,
				);
				return;
			}
			if (event.key === "Enter" && !event.shiftKey) {
				event.preventDefault();
				selectCommand(commandIndex);
				return;
			}
			if (event.key === "Escape") {
				event.preventDefault();
				setCommandsDismissed(true);
				return;
			}
		}
		if (event.key === "Escape") {
			event.preventDefault();
			input.current?.blur();
			announce("Left the composer");
			return;
		}
		if (event.key === "Tab" && event.shiftKey) {
			event.preventDefault();
			cycleMode();
		}
	}

	return (
		<PromptInput
			aria-label="Session composer"
			className="shrink-0 px-3 pb-3 pt-2"
			data-composer
			onSubmit={(event) => {
				event.preventDefault();
				if (needsName()) return;
				void post();
			}}
		>
			<HeldMessages
				organization={organization}
				workspace={workspace}
				held={read.held_messages}
				onDone={() => input.current?.focus()}
			/>
			{asking ? (
				<PromptInputHeader className="block">
					<div className="grid gap-1" data-name-gate>
						<Label htmlFor="composer-name">Your name</Label>
						<div className="flex gap-1">
							<Input
								id="composer-name"
								ref={nameInput}
								onChange={(event) => setDraftName(event.target.value)}
								onKeyDown={(event) => {
									if (event.key !== "Enter") return;
									event.preventDefault();
									remember();
								}}
								placeholder="jack"
								value={draftName}
							/>
							<Button onClick={remember} type="button">
								Use this name
							</Button>
						</div>
						{nameProblem && (
							<p className="text-destructive text-xs" role="alert">
								{nameProblem}
							</p>
						)}
						<p className="text-muted-foreground text-xs">
							The name is remembered in this browser and is never a password.
						</p>
					</div>
				</PromptInputHeader>
			) : (
				<>
					{/* oxlint-disable jsx-a11y/prefer-tag-over-role -- The textarea retains focus while the listbox exposes command hints through its active descendant. */}
					{matches.length > 0 && (
						<div
							id="harness-commands"
							role="listbox"
							aria-label="Harness commands"
							className="mb-2 w-full max-h-48 overflow-y-auto rounded-md border p-1"
						>
							{matches.map((command, index) => (
								<button
									type="button"
									tabIndex={-1}
									key={command.name}
									id={`harness-command-${index}`}
									role="option"
									aria-selected={index === commandIndex}
									className="block w-full rounded-sm px-2 py-1.5 text-left aria-selected:bg-accent"
									onMouseDown={(event) => event.preventDefault()}
									onClick={() => selectCommand(index)}
								>
									<span className="font-mono text-xs">/{command.name}</span>{" "}
									<span className="text-muted-foreground text-xs">{command.description}</span>
									{command.input_hint && (
										<p className="font-mono text-muted-foreground text-xs">{command.input_hint}</p>
									)}
								</button>
							))}
						</div>
					)}
					{/* oxlint-enable jsx-a11y/prefer-tag-over-role */}
					<PromptInputBody>
						<PromptInputTextarea
							aria-controls={matches.length > 0 ? "harness-commands" : undefined}
							aria-activedescendant={
								matches.length > 0 ? `harness-command-${commandIndex}` : undefined
							}
							aria-label={label}
							data-composer-input
							id="composer-input"
							onChange={(event) => {
								setDraft(event.target.value);
								setCommandIndex(0);
								setCommandsDismissed(false);
							}}
							onKeyDown={onKeyDown}
							placeholder={`${label} — / for commands`}
							ref={input}
							value={draft}
						/>
					</PromptInputBody>
					<PromptInputFooter className="flex-wrap">
						<SessionControls
							session={session}
							usage={live?.usage ?? session?.usage}
							optionWrite={optionWrite}
						/>
						<div className="flex w-full flex-wrap items-center gap-1">
							<Button disabled={busy || draft.trim().length === 0} type="submit">
								{label}
							</Button>
							{mayInterrupt(session) && (
								<Button
									disabled={busy || draft.trim().length === 0 || Boolean(session?.interrupting)}
									onClick={() => void sendNow()}
									type="button"
									variant="outline"
								>
									Send now
								</Button>
							)}
							<Button
								disabled={busy || !mayInterrupt(session) || Boolean(session?.interrupting)}
								onClick={() => void interrupt()}
								type="button"
								variant="outline"
							>
								{interruptPending || session?.interrupting ? "Interrupting…" : "Interrupt"}
							</Button>
							<span className="ml-auto text-muted-foreground text-xs" data-composer-name>
								{name === null ? "no name yet" : `writing as ${name}`}{" "}
								<Button
									onClick={() => {
										setAsking(true);
										setDraftName(name ?? "");
									}}
									size="xs"
									type="button"
									variant="link"
								>
									change
								</Button>
							</span>
						</div>
					</PromptInputFooter>
				</>
			)}
			{partial && (
				<Alert className="mt-2" variant="destructive">
					<AlertTitle>{partial}</AlertTitle>
				</Alert>
			)}
			{refusal !== null && (
				<div className="mt-2">
					<Refusal error={refusal} />
				</div>
			)}
			<p aria-live="polite" className="sr-only" data-announcement>
				{notice}
			</p>
		</PromptInput>
	);
}
