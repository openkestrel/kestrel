import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Textarea } from "#/components/ui/textarea";
import { declaredName, rememberDeclaredName } from "#/lib/declared-name";
import type { Workspace } from "#/operator/generated";
import { postMessage } from "#/operator/getting-ready";
import { workspaceKey } from "#/operator/queries";

// The least a person needs to send the Brief. The composer ticket owns turns, held-message edits,
// interruption and options; this only writes a message and shows what is held.
export function BriefComposer({
	organization,
	record,
}: {
	organization: string;
	record: Workspace | undefined;
}) {
	const queryClient = useQueryClient();
	const [message, setMessage] = useState("");
	const [name, setName] = useState(() => declaredName());

	const post = useMutation({
		mutationFn: () => postMessage(organization, record?.name ?? "", name.trim(), message.trim()),
		onSuccess: () => {
			rememberDeclaredName(name);
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
			{record.held_messages.length > 0 && (
				<section data-held className="grid gap-1 text-xs">
					<h2 className="text-muted-foreground">Held until the Session is ready</h2>
					<ul className="grid gap-0.5">
						{record.held_messages.map((held) => (
							<li key={held.id} data-held-message>
								<span className="font-medium">{held.participant}:</span> {held.message}
							</li>
						))}
					</ul>
				</section>
			)}
			<form onSubmit={submit} className="grid gap-2">
				{name.trim() === "" && (
					<Input
						aria-label="Your name"
						placeholder="Your name"
						value={name}
						onChange={(event) => setName(event.target.value)}
					/>
				)}
				<Textarea
					aria-label="Message"
					placeholder="Write the Brief…"
					value={message}
					onChange={(event) => setMessage(event.target.value)}
				/>
				<div className="flex items-center justify-between gap-2">
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
				</div>
			</form>
			{post.isError && <Refusal error={post.error} />}
		</div>
	);
}
