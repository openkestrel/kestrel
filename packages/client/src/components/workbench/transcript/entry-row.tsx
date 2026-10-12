import { CircleCheckIcon, CircleXIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useId } from "react";
import { MessageResponse } from "#/components/ai-elements/message";
import { cn } from "#/lib/utils";
import type { Exit } from "#/operator/generated";
import type { Delivered } from "#/operator/transcript";
import { at, exitLine, spoken, type Spoken } from "#/operator/transcript-view";
import type { Row } from "../row";
import { PayloadText } from "./payload-text";
import { type Disclosure, PlanStep, ThoughtStep, ToolStep } from "./step-row";

export type { Disclosure };

export function EntryRow({ entry, mode, ...row }: { entry: Delivered; mode: Disclosure } & Row) {
	return (
		<div data-seq={entry.seq} className="py-0.5 text-sm">
			{content(entry, mode, row)}
		</div>
	);
}

function content(entry: Delivered, mode: Disclosure, row: Row): ReactNode {
	const value = entry.entry;
	switch (value.type) {
		case "said":
			return spoken(entry).map((message) => (
				<Message key={message.key} message={message} appendedAt={entry.appendedAt} {...row} />
			));
		case "messages":
			return Array.isArray(value.messages) ? (
				<ul className="grid gap-3">
					{spoken(entry).map((message) => (
						<li key={message.key}>
							<Message message={message} appendedAt={entry.appendedAt} {...row} />
						</li>
					))}
				</ul>
			) : (
				<PayloadText {...row} reference={value.messages} />
			);
		case "brief":
			return (
				<div className="text-muted-foreground">
					{typeof value.brief === "string" ? (
						<MessageResponse>{value.brief}</MessageResponse>
					) : (
						<PayloadText {...row} reference={value.brief} />
					)}
				</div>
			);
		case "participant_joined":
			return <p className="text-muted-foreground text-xs">{value.participant} joined</p>;
		case "session_started":
			return <p className="text-muted-foreground text-xs">{value.agent} started</p>;
		case "session_ended":
			return <SessionEnded exit={value.exit} />;
		case "turn_interrupted":
			return <p className="text-muted-foreground text-xs">Interrupted by {value.participant}</p>;
		case "option_changed":
			return (
				<p className="text-muted-foreground text-xs">
					{value.refused
						? `Option ${value.option} refused: ${value.refused}`
						: `Option ${value.option}: ${value.from ?? "default"} → ${value.to ?? "default"}`}
				</p>
			);
		case "instance_released":
			return <p className="text-muted-foreground text-xs">Instance released</p>;
		case "expired":
			return <p className="text-muted-foreground text-xs">expired {at(value.expired_at) ?? ""}</p>;
		case "pull_request":
			return (
				<p>
					<a href={value.url} className="underline" rel="noreferrer" target="_blank">
						Pull request {value.number}: {value.title}
					</a>{" "}
					<span className="text-muted-foreground text-xs">{value.state}</span>
				</p>
			);
		case "thought":
			return <ThoughtStep entry={value} mode={mode} {...row} />;
		case "plan":
			return <PlanStep entry={value} mode={mode} {...row} />;
		case "tool_call":
			return <ToolStep entry={value} mode={mode} {...row} />;
		default: {
			// Unknown shared-state kinds are shown plainly rather than taking the pane down.
			const kind = (value as { type?: unknown }).type;
			return (
				<p className="text-muted-foreground text-xs">
					{typeof kind === "string" ? kind : "unknown entry"}
				</p>
			);
		}
	}
}

function Message({ message, appendedAt, ...row }: { message: Spoken; appendedAt: string } & Row) {
	const name = useId();
	const agent = message.speaker === "agent";
	return (
		<article aria-labelledby={name} className="min-w-0">
			<p className="mb-1 flex items-baseline gap-2 text-xs">
				<span id={name} className={cn("font-medium", agent && "text-muted-foreground")}>
					{message.participant}
				</span>
				<time dateTime={appendedAt} className="text-muted-foreground">
					{at(appendedAt)}
				</time>
			</p>
			<div
				className={cn(
					"min-w-0 text-sm",
					agent ? "leading-relaxed" : "w-fit max-w-5/6 rounded-lg bg-muted px-3 py-2",
				)}
			>
				{typeof message.body === "string" ? (
					<MessageResponse>{message.body}</MessageResponse>
				) : (
					<PayloadText {...row} reference={message.body} />
				)}
			</div>
		</article>
	);
}

function SessionEnded({ exit }: { exit: Exit }) {
	const failed = exit.status === "failed";
	const Icon = failed ? CircleXIcon : CircleCheckIcon;
	return (
		<p className="flex items-center gap-1.5 py-1 text-xs">
			<Icon aria-hidden className={cn("size-3.5 shrink-0", failed && "text-destructive")} />
			<span className={failed ? "font-medium" : "text-muted-foreground"}>{exitLine(exit)}</span>
			<span aria-hidden className="h-px min-w-4 flex-1 bg-border" />
		</p>
	);
}
