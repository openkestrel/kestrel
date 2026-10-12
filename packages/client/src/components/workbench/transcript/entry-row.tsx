import { CircleCheckIcon, CircleXIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useId, useState } from "react";
import { MessageResponse } from "#/components/ai-elements/message";
import { Button } from "#/components/ui/button";
import { ScrollablePre } from "#/components/workbench/scrollable-pre";
import { cn } from "#/lib/utils";
import type { Delivered } from "#/operator/transcript";
import {
	at,
	elapsed,
	exitCode,
	exitLine,
	firstLine,
	payloadReference,
	planStep,
	spoken,
	type Spoken,
	toolState,
} from "#/operator/transcript-view";
import { PayloadText } from "./payload-text";

export type Disclosure = "line" | "steps" | "full";

type Row = { organization: string; workspace: string };

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
		case "thought": {
			if (typeof value.text !== "string") return <PayloadText {...row} reference={value.text} />;
			const text = value.text;
			return (
				<Expandable
					mode={mode}
					summary={(open) =>
						open ? (
							<div className="text-muted-foreground">
								<MessageResponse>{text}</MessageResponse>
							</div>
						) : (
							<p className="text-muted-foreground italic">{firstLine(text)}</p>
						)
					}
					detail={null}
				/>
			);
		}
		case "plan":
			return Array.isArray(value.entries) ? (
				<Expandable
					mode={mode}
					summary={<p className="text-muted-foreground">Plan: {value.entries.length} steps</p>}
					detail={
						<ol className="grid gap-1 text-xs">
							{withKeys(value.entries, (step) => planStep(step)?.content ?? "step").map(
								({ item: step, key }) => {
									const parsed = planStep(step);
									return (
										<li key={`${entry.seq}:${key}`}>
											<span className="text-muted-foreground">{parsed?.status ?? "pending"}</span>{" "}
											{parsed?.content ?? "a step"}
										</li>
									);
								},
							)}
						</ol>
					}
				/>
			) : (
				<PayloadText {...row} reference={value.entries} />
			);
		case "tool_call":
			return <ToolRow entry={value} mode={mode} {...row} />;
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

function SessionEnded({
	exit,
}: {
	exit: Extract<Delivered["entry"], { type: "session_ended" }>["exit"];
}) {
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

function Expandable({
	mode,
	summary,
	detail,
}: {
	mode: Disclosure;
	summary: ReactNode | ((open: boolean) => ReactNode);
	detail: ReactNode;
}) {
	const [override, setOverride] = useState<boolean | undefined>(undefined);
	const open = override ?? mode === "full";

	return (
		<div className="grid gap-1">
			<div className="flex items-start gap-2">
				<div className="min-w-0 flex-1">
					{typeof summary === "function" ? summary(open) : summary}
				</div>
				<Button
					type="button"
					variant="ghost"
					size="sm"
					aria-expanded={open}
					onClick={() => setOverride(!open)}
				>
					{open ? "Less" : "More"}
				</Button>
			</div>
			{open && detail}
		</div>
	);
}

function ToolRow({
	entry,
	mode,
	...row
}: { entry: Extract<Delivered["entry"], { type: "tool_call" }>; mode: Disclosure } & Row) {
	const state = toolState(entry.status, entry.closing_reason);
	const duration = elapsed(
		Date.parse(entry.completion.finished_at) - Date.parse(entry.completion.started_at),
	);
	const code = exitCode(entry.result);

	return (
		<Expandable
			mode={mode}
			summary={
				<div className="flex flex-wrap items-baseline gap-x-2">
					<span
						className={
							state === "running"
								? "text-muted-foreground text-xs"
								: state === "completed"
									? "text-xs"
									: "text-destructive text-xs"
						}
					>
						{state}
					</span>
					<span className="font-medium">{entry.title}</span>
					<span className="text-muted-foreground text-xs">
						{at(entry.completion.started_at) ?? ""}
						{duration ? ` · ${duration}` : ""}
					</span>
					{code !== undefined && <span className="text-muted-foreground text-xs">exit {code}</span>}
				</div>
			}
			detail={
				<div className="grid gap-2 border-l pl-2">
					<Field label="Input" value={entry.input} {...row} />
					<Field label="Result" value={entry.result} {...row} />
				</div>
			}
		/>
	);
}

function Field({ label, value, ...row }: { label: string; value: unknown } & Row) {
	const reference = payloadReference(value);
	return (
		<div className="grid gap-1">
			<span className="text-muted-foreground text-xs">{label}</span>
			{reference ? (
				<PayloadText {...row} reference={reference} />
			) : (
				<ScrollablePre className="max-h-64 overflow-auto rounded-md border p-2 text-xs">
					{JSON.stringify(value, null, 2)}
				</ScrollablePre>
			)}
		</div>
	);
}

function withKeys<T>(items: T[], key: (item: T) => string): { item: T; key: string }[] {
	const seen = new Map<string, number>();
	return items.map((item) => {
		const base = key(item);
		const count = seen.get(base) ?? 0;
		seen.set(base, count + 1);
		return { item, key: `${base}#${count}` };
	});
}
