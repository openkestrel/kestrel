import type { ReactNode } from "react";
import { useState } from "react";
import { Button } from "#/components/ui/button";
import type { Delivered } from "#/operator/transcript";
import {
	at,
	elapsed,
	exitCode,
	firstLine,
	payloadReference,
	planStep,
	toolState,
} from "#/operator/transcript-view";
import { PayloadText } from "./payload-text";

export type Disclosure = "line" | "steps" | "full";

type Row = { organization: string; workspace: string };

export function EntryRow({ entry, mode, ...row }: { entry: Delivered; mode: Disclosure } & Row) {
	return (
		<article data-seq={entry.seq} className="py-0.5 text-sm">
			{content(entry, mode, row)}
		</article>
	);
}

function content(entry: Delivered, mode: Disclosure, row: Row): ReactNode {
	const value = entry.entry;
	switch (value.type) {
		case "said":
			return (
				<p>
					<span className="font-medium">{value.participant}:</span>{" "}
					{typeof value.message === "string" ? (
						value.message
					) : (
						<PayloadText {...row} reference={value.message} />
					)}
				</p>
			);
		case "messages":
			return Array.isArray(value.messages) ? (
				<ul className="grid gap-1">
					{withKeys(value.messages, (message) => `${entry.seq}:${message.participant}`).map(
						({ item: message, key }) => (
							<li key={key}>
								<span className="font-medium">{message.participant}:</span>{" "}
								{typeof message.message === "string" ? (
									message.message
								) : (
									<PayloadText {...row} reference={message.message} />
								)}
							</li>
						),
					)}
				</ul>
			) : (
				<PayloadText {...row} reference={value.messages} />
			);
		case "brief":
			return (
				<div className="text-muted-foreground">
					{typeof value.brief === "string" ? (
						value.brief
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
			return <p className="text-muted-foreground text-xs">Session ended</p>;
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
			const text =
				typeof value.text === "string" ? (
					mode === "full" ? (
						value.text
					) : (
						firstLine(value.text)
					)
				) : (
					<PayloadText {...row} reference={value.text} />
				);
			return (
				<Expandable
					mode={mode}
					summary={<p className="text-muted-foreground">{text}</p>}
					detail={
						typeof value.text === "string" ? (
							<pre className="max-h-64 overflow-auto whitespace-pre-wrap text-xs">{value.text}</pre>
						) : null
					}
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
			// Approvals, questions and reports arrive as new shared-state kinds; show them plainly
			// rather than taking the pane down.
			const kind = (value as { type?: unknown }).type;
			return (
				<p className="text-muted-foreground text-xs">
					{typeof kind === "string" ? kind : "unknown entry"}
				</p>
			);
		}
	}
}

function Expandable({
	mode,
	summary,
	detail,
}: {
	mode: Disclosure;
	summary: ReactNode;
	detail: ReactNode;
}) {
	const [override, setOverride] = useState<boolean | undefined>(undefined);
	const open = override ?? mode === "full";

	return (
		<div className="grid gap-1">
			<div className="flex items-start gap-2">
				<div className="min-w-0 flex-1">{summary}</div>
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
				<pre className="max-h-64 overflow-auto rounded-md border p-2 text-xs">
					{JSON.stringify(value, null, 2)}
				</pre>
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
