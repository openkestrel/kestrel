import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useRef, useState } from "react";
import {
	PromptInput,
	PromptInputBody,
	PromptInputTextarea,
	PromptInputFooter,
} from "#/components/ai-elements/prompt-input";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import type { Session, Usage, Workspace } from "#/operator/generated";
import { postMessage } from "#/operator/getting-ready";
import { participant, useParticipant } from "#/operator/participant";
import { workspaceKey } from "#/operator/queries";
import { HeldMessages } from "./held-messages";
import type { OptionWrite } from "./option-write";
import { SessionControls } from "./session-controls";

export function BriefComposer({
	organization,
	record,
	briefed,
	session,
	usage,
	optionWrite,
}: {
	organization: string;
	record: Workspace | undefined;
	// Still rendered once briefed so the field a person is typing into is not torn out mid-handoff.
	briefed: boolean;
	session: Session | undefined;
	usage: Usage | null | undefined;
	optionWrite: OptionWrite;
}) {
	const queryClient = useQueryClient();
	const knownName = useParticipant();
	const input = useRef<HTMLTextAreaElement>(null);
	const [message, setMessage] = useState("");
	const [name, setName] = useState(() => participant.name() ?? "");

	const post = useMutation({
		mutationFn: () => postMessage(organization, record?.name ?? "", name.trim(), message.trim()),
		onSuccess: () => {
			participant.remember(name);
			setMessage("");
			if (record) {
				void queryClient.invalidateQueries({
					queryKey: workspaceKey(organization, record.name),
				});
			}
		},
	});

	if (!record || record.state !== "open") return null;

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		if (message.trim() === "" || name.trim() === "") return;
		post.mutate();
	};

	return (
		<div className="grid shrink-0 gap-2 border-t p-3">
			<PromptInput aria-label="Brief composer" onSubmit={submit}>
				{!briefed && (
					<HeldMessages
						organization={organization}
						workspace={record.name}
						held={record.held_messages}
						onDone={() => input.current?.focus()}
					/>
				)}
				{!briefed && knownName === null && (
					<Input
						aria-label="Your name"
						placeholder="Your name"
						value={name}
						onChange={(event) => setName(event.target.value)}
					/>
				)}
				<PromptInputBody>
					<PromptInputTextarea
						ref={input}
						aria-label="Message"
						placeholder="Write the Brief…"
						value={message}
						onChange={(event) => setMessage(event.target.value)}
					/>
				</PromptInputBody>
				<PromptInputFooter className="flex-wrap">
					{!briefed && (
						<SessionControls session={session} usage={usage} optionWrite={optionWrite} />
					)}
					<span className="text-muted-foreground text-xs">
						Your first message becomes the Brief; one sent while the Session is getting ready is
						held.
					</span>
					<Button
						type="submit"
						size="sm"
						disabled={post.isPending || message.trim() === "" || name.trim() === ""}
					>
						{post.isPending ? "Sending…" : "Send"}
					</Button>
				</PromptInputFooter>
			</PromptInput>
			{post.isError && <Refusal error={post.error} />}
		</div>
	);
}
