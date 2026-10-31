import {
	BrainIcon,
	FileTextIcon,
	ListTodoIcon,
	PencilIcon,
	SearchIcon,
	SquareTerminalIcon,
	WrenchIcon,
	XIcon,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { CodeBlockContainer, CodeBlockContent } from "#/components/ai-elements/code-block";
import { MessageResponse } from "#/components/ai-elements/message";
import {
	Queue,
	QueueItem,
	QueueItemContent,
	QueueItemIndicator,
} from "#/components/ai-elements/queue";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Terminal, TerminalContent } from "#/components/ai-elements/terminal";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { useNow } from "#/lib/use-now";
import { cn } from "#/lib/utils";
import type { Entry, RunningTool } from "#/operator/generated";
import {
	elapsed,
	exitCode,
	payloadReference,
	phrase,
	planStep,
	thoughtLine,
	toolOutput,
	toolState,
} from "#/operator/transcript-view";
import type { Row } from "../row";
import { PayloadText } from "./payload-text";

export type Disclosure = "line" | "steps" | "full";

type Recorded<Type extends Entry["type"]> = Extract<Entry, { type: Type }>;

const ICON = "size-3.5 shrink-0";

const KIND_ICONS = new Map([
	["read", FileTextIcon],
	["edit", PencilIcon],
	["search", SearchIcon],
	["execute", SquareTerminalIcon],
]);

export function KindIcon({ kind, label }: { kind: string; label?: string }) {
	const Icon = KIND_ICONS.get(kind) ?? WrenchIcon;
	return (
		<Icon
			aria-hidden={label === undefined ? true : undefined}
			role={label === undefined ? undefined : "img"}
			aria-label={label}
			className={ICON}
		/>
	);
}

function Line({
	icon,
	label,
	trailing,
	failed = false,
}: {
	icon: ReactNode;
	label: ReactNode;
	trailing?: ReactNode;
	failed?: boolean;
}) {
	return (
		<span
			className={cn(
				"flex min-w-0 items-center gap-2 px-1.5 py-1 text-left text-sm",
				failed ? "text-destructive" : "text-muted-foreground",
			)}
		>
			{icon}
			<span className="min-w-0 flex-1 truncate">{label}</span>
			{trailing && <span className="shrink-0 text-xs tabular-nums">{trailing}</span>}
		</span>
	);
}

function Step({
	name,
	mode,
	children,
	...line
}: {
	name: string;
	mode: Disclosure;
	children: ReactNode;
} & Parameters<typeof Line>[0]) {
	const [override, setOverride] = useState<boolean | undefined>(undefined);
	const open = override ?? mode === "full";

	return (
		<Collapsible open={open} onOpenChange={setOverride}>
			<CollapsibleTrigger aria-label={name} className="w-full">
				<Line {...line} />
			</CollapsibleTrigger>
			<CollapsibleContent>
				<div className="my-1 ml-6 grid min-w-0 gap-2">{children}</div>
			</CollapsibleContent>
		</Collapsible>
	);
}

// One component from running to recorded, so the row a call completes in is the row it ran in.
export function ToolStep({
	running,
	entry,
	mode,
	...row
}: { running?: RunningTool; entry?: Recorded<"tool_call">; mode: Disclosure } & Row) {
	if (entry) return <CalledStep entry={entry} mode={mode} {...row} />;
	return running ? <RunningStep tool={running} /> : null;
}

function RunningStep({ tool }: { tool: RunningTool }) {
	const now = useNow(1000);

	return (
		<Line
			icon={<KindIcon kind={tool.tool_kind} />}
			label={
				<>
					<Title kind={tool.tool_kind}>
						<Shimmer>{tool.title}</Shimmer>
					</Title>
					<span className="sr-only">, running</span>
				</>
			}
			trailing={elapsed(now - Date.parse(tool.started_at))}
		/>
	);
}

function Title({ kind, children }: { kind: string; children: ReactNode }) {
	return kind === "execute" ? <span className="font-mono text-xs">{children}</span> : children;
}

function CalledStep({
	entry,
	mode,
	...row
}: { entry: Recorded<"tool_call">; mode: Disclosure } & Row) {
	const state = toolState(entry.status, entry.closing_reason);
	const failed = state !== "completed" && state !== "running";
	const duration = took(entry.completion);
	const code = exitCode(entry.result);
	const exit = code === undefined ? undefined : `exit ${code}`;

	return (
		<Step
			name={phrase(entry.title, state, exit, duration)}
			mode={mode}
			icon={failed ? <XIcon aria-hidden className={ICON} /> : <KindIcon kind={entry.tool_kind} />}
			label={<Title kind={entry.tool_kind}>{entry.title}</Title>}
			trailing={[exit, failed ? state : duration].filter(Boolean).join(" · ")}
			failed={failed}
		>
			<Input value={entry.input} {...row} />
			<Output entry={entry} failed={state === "failed"} {...row} />
		</Step>
	);
}

function Input({ value, ...row }: { value: unknown } & Row) {
	const reference = payloadReference(value);
	if (reference) return <PayloadText {...row} reference={reference} />;
	if (value === null || value === undefined) return null;
	if (typeof value === "object" && Object.keys(value).length === 0) return null;
	return <Code label="Input" language="json" code={JSON.stringify(value, null, 2)} />;
}

function Output({
	entry,
	failed,
	...row
}: { entry: Recorded<"tool_call">; failed: boolean } & Row) {
	const reference = payloadReference(entry.result);
	if (reference) return <PayloadText {...row} reference={reference} />;
	const output = toolOutput(entry.result);
	if (!output) return <p className="text-muted-foreground text-xs">No output</p>;
	if (failed) {
		return (
			<p role="alert" className="font-mono text-destructive text-xs whitespace-pre-wrap">
				{output.text}
			</p>
		);
	}
	if (output.format === "text" && entry.tool_kind === "execute") {
		return (
			<Terminal output={output.text} autoScroll={false}>
				<TerminalContent
					aria-label="Output"
					// oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- the component owns the scrolling element.
					role="region"
					tabIndex={0}
					className="max-h-64 p-2 text-xs"
				/>
			</Terminal>
		);
	}
	return <Code label="Output" language={output.format} code={output.text} />;
}

function Code({
	label,
	language,
	code,
}: {
	label: string;
	language: "json" | "text";
	code: string;
}) {
	return (
		<CodeBlockContainer language={language}>
			<CodeBlockContent
				aria-label={label}
				// oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- the component owns the scrolling element.
				role="region"
				className="max-h-64"
				code={code}
				language={language}
			/>
		</CodeBlockContainer>
	);
}

export function ThoughtStep({
	entry,
	mode,
	...row
}: { entry: Recorded<"thought">; mode: Disclosure } & Row) {
	const text = entry.text;
	const line = typeof text === "string" ? thoughtLine(text) : undefined;
	const duration = took(entry.completion);

	return (
		<Step
			name={phrase("Thought", line, duration)}
			mode={mode}
			icon={<BrainIcon aria-hidden className={ICON} />}
			label={<span className="italic">{line ?? "Thought"}</span>}
			trailing={duration}
		>
			{typeof text === "string" ? (
				<div className="text-muted-foreground text-sm">
					<MessageResponse>{text}</MessageResponse>
				</div>
			) : (
				<PayloadText {...row} reference={text} />
			)}
		</Step>
	);
}

export function PlanStep({
	entry,
	mode,
	...row
}: { entry: Recorded<"plan">; mode: Disclosure } & Row) {
	const steps = Array.isArray(entry.entries) ? entry.entries : undefined;
	const count = steps ? `${steps.length} ${steps.length === 1 ? "step" : "steps"}` : undefined;

	return (
		<Step
			name={phrase("Plan", count)}
			mode={mode}
			icon={<ListTodoIcon aria-hidden className={ICON} />}
			label={count ? `Plan: ${count}` : "Plan"}
		>
			{Array.isArray(entry.entries) ? (
				<Queue>
					<ul aria-label="Plan">
						{withKeys(entry.entries, (step) => planStep(step)?.content ?? "step").map(
							({ item, key }) => {
								const step = planStep(item);
								const status = (step?.status ?? "pending").replaceAll("_", " ");
								return (
									<QueueItem key={key}>
										<div className="flex items-center gap-2">
											<QueueItemIndicator completed={status === "completed"} />
											<QueueItemContent completed={status === "completed"}>
												<span className="sr-only">{status}: </span>
												{step?.content ?? "a step"}
											</QueueItemContent>
										</div>
									</QueueItem>
								);
							},
						)}
					</ul>
				</Queue>
			) : (
				<PayloadText {...row} reference={entry.entries} />
			)}
		</Step>
	);
}

function took(completion: { started_at: string; finished_at: string }): string | undefined {
	return elapsed(Date.parse(completion.finished_at) - Date.parse(completion.started_at));
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
