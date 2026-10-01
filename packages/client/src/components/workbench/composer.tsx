import { useQueryClient } from "@tanstack/react-query";
import { type KeyboardEvent, useRef, useState } from "react";
import { Refusal } from "#/components/refusal";
import { Alert, AlertTitle } from "#/components/ui/alert";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { Textarea } from "#/components/ui/textarea";
import {
	amendable,
	heldAge,
	mayWriteOptions,
	modeOption,
	nextMode,
	optionChange,
	partialReport,
	postLabel,
	wasAmended,
} from "#/operator/composer";
import type { HeldMessage, Session, Workspace } from "#/operator/generated";
import { participant, useParticipant } from "#/operator/participant";
import { operator, sessionsKey, workspaceKey } from "#/operator/queries";
import { mayInterrupt } from "#/operator/session-view";
import {
	changeSessionOption,
	editHeldMessage,
	interruptTurn,
	postTurn,
	withdrawHeldMessage,
} from "#/operator/turns";

export function Composer({
	organization,
	workspace,
	read,
	session,
}: {
	organization: string;
	workspace: string;
	read: Workspace;
	session: Session | undefined;
}) {
	const name = useParticipant();
	const queryClient = useQueryClient();
	const input = useRef<HTMLTextAreaElement>(null);
	const [draft, setDraft] = useState("");
	const [asking, setAsking] = useState(false);
	const [draftName, setDraftName] = useState("");
	const [nameProblem, setNameProblem] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const [refusal, setRefusal] = useState<unknown>(null);
	const [partial, setPartial] = useState<string | null>(null);
	const [notice, setNotice] = useState("");
	const [editing, setEditing] = useState<{ id: number; message: string } | null>(null);

	const label = postLabel(session?.state);
	const held = read.held_messages;

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
	}

	async function post(): Promise<boolean> {
		if (name === null || draft.trim().length === 0) return false;
		setBusy(true);
		setRefusal(null);
		setPartial(null);
		try {
			const posted = await postTurn(operator, organization, workspace, name, draft.trim());
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
			await interruptTurn(operator, organization, session.id, name);
			await refresh();
		} catch (error) {
			setRefusal(error);
			setPartial(partialReport(true, false) ?? null);
		} finally {
			setBusy(false);
		}
	}

	async function interrupt() {
		if (needsName() || name === null || !session) return;
		setBusy(true);
		setRefusal(null);
		setPartial(null);
		try {
			await interruptTurn(operator, organization, session.id, name);
			await refresh();
		} catch (error) {
			setRefusal(error);
		} finally {
			setBusy(false);
		}
	}

	async function saveEdit(message: HeldMessage) {
		if (name === null || !editing) return;
		setBusy(true);
		setRefusal(null);
		try {
			await editHeldMessage(operator, organization, workspace, message.id, name, editing.message);
			setEditing(null);
			await refresh();
		} catch (error) {
			setRefusal(error);
		} finally {
			setBusy(false);
		}
	}

	async function withdraw(message: HeldMessage) {
		if (name === null) return;
		setBusy(true);
		setRefusal(null);
		try {
			await withdrawHeldMessage(operator, organization, workspace, message.id, name);
			await refresh();
		} catch (error) {
			setRefusal(error);
		} finally {
			setBusy(false);
		}
	}

	function cycleMode() {
		const option = session ? modeOption(session.options) : undefined;
		if (!option) {
			announce("This harness offers no mode to cycle");
			return;
		}
		if (!mayWriteOptions(session?.state)) {
			announce("The mode cannot change during a working turn");
			return;
		}
		const next = nextMode(option, String(option.current));
		if (!next) {
			announce("This harness offers no mode to cycle");
			return;
		}
		if (name === null || !session) return;
		announce(`mode ${next.value}`);
		changeSessionOption(operator, organization, session.id, optionChange(option, next.value, name))
			.then(() => refresh())
			.catch((error: unknown) => {
				setRefusal(error);
			});
	}

	function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
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
		<form
			className="shrink-0 border-t px-4 py-2"
			data-composer
			onSubmit={(event) => {
				event.preventDefault();
				if (needsName()) return;
				void post();
			}}
		>
			{held.length > 0 && (
				<ul aria-label="Held Messages" className="mb-2 grid gap-1" data-held-messages>
					{held.map((message) => (
						<li
							key={message.id}
							className="border border-border p-2 text-xs"
							data-held-message={message.id}
						>
							<div className="flex flex-wrap items-center gap-2">
								<span className="font-medium">{message.participant}</span>
								<span className="text-muted-foreground">{heldAge(message.posted_at)}</span>
								{wasAmended(message) && <span className="text-muted-foreground">edited</span>}
								{amendable(message, name) && (
									<span className="ml-auto flex gap-1">
										<Button
											onClick={() => setEditing({ id: message.id, message: message.message })}
											size="xs"
											type="button"
											variant="outline"
										>
											Edit
										</Button>
										<Button
											disabled={busy}
											onClick={() => void withdraw(message)}
											size="xs"
											type="button"
											variant="outline"
										>
											Withdraw
										</Button>
									</span>
								)}
							</div>
							{editing?.id === message.id ? (
								<div className="mt-1 grid gap-1">
									<Textarea
										aria-label="Edit the held message"
										onChange={(event) =>
											setEditing({ id: message.id, message: event.target.value })
										}
										value={editing.message}
									/>
									<div className="flex gap-1">
										<Button
											disabled={busy}
											onClick={() => void saveEdit(message)}
											size="xs"
											type="button"
										>
											Save
										</Button>
										<Button
											onClick={() => setEditing(null)}
											size="xs"
											type="button"
											variant="ghost"
										>
											Cancel
										</Button>
									</div>
								</div>
							) : (
								<p className="mt-1" data-held-text>
									{message.message}
								</p>
							)}
						</li>
					))}
				</ul>
			)}
			{asking ? (
				<div className="grid gap-1" data-name-gate>
					<Label htmlFor="composer-name">Your name</Label>
					<div className="flex gap-1">
						<Input
							id="composer-name"
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
			) : (
				<>
					<Textarea
						aria-label={label}
						data-composer-input
						onChange={(event) => setDraft(event.target.value)}
						onKeyDown={onKeyDown}
						placeholder={label}
						ref={input}
						value={draft}
					/>
					<div className="mt-1 flex flex-wrap items-center gap-1">
						<Button disabled={busy || draft.trim().length === 0} type="submit">
							{label}
						</Button>
						{mayInterrupt(session) && (
							<Button
								disabled={busy || draft.trim().length === 0}
								onClick={() => void sendNow()}
								type="button"
								variant="outline"
							>
								Send now
							</Button>
						)}
						<Button
							disabled={busy || !mayInterrupt(session)}
							onClick={() => void interrupt()}
							type="button"
							variant="outline"
						>
							Interrupt
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
		</form>
	);
}
