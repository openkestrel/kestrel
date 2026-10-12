import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { PromptInputHeader } from "#/components/ai-elements/prompt-input";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Textarea } from "#/components/ui/textarea";
import { amendable, wasAmended } from "#/operator/composer";
import { ago } from "#/operator/format";
import type { HeldMessage } from "#/operator/generated";
import { useParticipant } from "#/operator/participant";
import { operator, sessionsKey, workspaceKey } from "#/operator/queries";
import { editHeldMessage, withdrawHeldMessage } from "#/operator/turns";

export function HeldMessages({
	organization,
	workspace,
	held,
	onDone,
}: {
	organization: string;
	workspace: string;
	held: HeldMessage[];
	onDone: () => void;
}) {
	const name = useParticipant();
	const editInput = useRef<HTMLTextAreaElement>(null);
	const queryClient = useQueryClient();
	const [busy, setBusy] = useState(false);
	const [refusal, setRefusal] = useState<unknown>(null);
	const [editing, setEditing] = useState<{ id: number; message: string } | null>(null);
	const editingId = editing?.id;
	useEffect(() => {
		if (editingId !== undefined) editInput.current?.focus();
	}, [editingId]);
	async function refresh() {
		await queryClient.invalidateQueries({ queryKey: workspaceKey(organization, workspace) });
		await queryClient.invalidateQueries({ queryKey: sessionsKey(organization) });
	}
	async function saveEdit(message: HeldMessage) {
		if (name === null || !editing) return;
		setBusy(true);
		setRefusal(null);
		try {
			await editHeldMessage(operator, organization, workspace, message.id, name, editing.message);
			setEditing(null);
			await refresh();
			onDone();
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
			onDone();
		} catch (error) {
			setRefusal(error);
		} finally {
			setBusy(false);
		}
	}

	return (
		<>
			{held.length > 0 && (
				<PromptInputHeader className="block">
					<p className="mb-1 text-muted-foreground text-xs">Held for the next Turn</p>
					<ul aria-label="Held Messages" className="mb-2 grid gap-1" data-held-messages>
						{held.map((message) => (
							<li
								key={message.id}
								className="rounded-md bg-muted/60 px-2 py-1.5 text-xs"
								data-held-message={message.id}
							>
								<div className="flex flex-wrap items-center gap-2">
									<span className="font-medium">{message.participant}</span>
									<span className="text-muted-foreground">{ago(message.posted_at)}</span>
									{wasAmended(message) && <span className="text-muted-foreground">edited</span>}
									{amendable(message, name) && (
										<span className="ml-auto flex gap-1">
											<Button
												disabled={busy}
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
											ref={editInput}
											aria-label="Edit the held message"
											onChange={(event) =>
												setEditing({ id: message.id, message: event.target.value })
											}
											value={editing.message}
										/>
										<div className="flex gap-1">
											<Button
												disabled={busy || editing.message.trim().length === 0}
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
				</PromptInputHeader>
			)}
			{refusal !== null && <Refusal error={refusal} />}
		</>
	);
}
