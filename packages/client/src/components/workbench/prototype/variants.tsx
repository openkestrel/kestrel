// PROTOTYPE (#492): three structurally different Transcript panes on AI Elements.
// A "Cards": every Activity open, one Tool or Reasoning per entry, bubbles, controls in the header.
// B "Steps": everyone flush left, each Activity a Chain of Thought, controls in the composer.
// C "Folded": each Activity one line until opened, controls in a slim header.
import { BrainIcon, ChevronRightIcon, ListTodoIcon, XCircleIcon } from "lucide-react";
import { useState } from "react";
import {
	ChainOfThought,
	ChainOfThoughtContent,
	ChainOfThoughtHeader,
	ChainOfThoughtStep,
} from "#/components/ai-elements/chain-of-thought";
import {
	Conversation,
	ConversationContent,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { cn } from "#/lib/utils";
import { elapsed } from "#/operator/transcript-view";
import { VariantD } from "./variant-d";
import { MainE } from "./variant-e";
import { type ActivityGroup, FLOW, type Narrated, RUNNING } from "./fixtures";
import {
	BubbleMessage,
	HeldQueue,
	KestrelComposer,
	LogMessage,
	ModelPicker,
	NoticeLine,
	PlanView,
	RunningToolView,
	SessionMeta,
	SessionTitle,
	ThoughtView,
	ToolCallView,
	ToolTitle,
	UsageContext,
	useTicking,
} from "./parts";

function activityLine(activity: ActivityGroup): string {
	const { tools, failed, thoughts, plans } = activity.counts;
	const parts = [
		tools && `${tools} ${tools === 1 ? "tool" : "tools"}`,
		failed && `${failed} failed`,
		thoughts && `${thoughts} ${thoughts === 1 ? "thought" : "thoughts"}`,
		plans && `${plans} ${plans === 1 ? "plan" : "plans"}`,
	].filter(Boolean);
	return parts.join(", ");
}

function workedFor(activity: ActivityGroup): string | undefined {
	return activity.finishedAt
		? elapsed(Date.parse(activity.finishedAt) - Date.parse(activity.startedAt))
		: undefined;
}

function NarratedView({ entry }: { entry: Narrated }) {
	switch (entry.kind) {
		case "tool":
			return <ToolCallView call={entry} />;
		case "thought":
			return <ThoughtView thought={entry} />;
		case "plan":
			return <PlanView plan={entry} />;
	}
}

export function VariantA() {
	return (
		<>
			<header className="grid shrink-0 gap-2 border-b px-4 py-3">
				<SessionTitle />
				<SessionMeta />
				<div className="flex flex-wrap items-center gap-2">
					<ModelPicker />
					<UsageContext />
				</div>
			</header>
			<Conversation>
				<ConversationContent className="gap-4">
					{FLOW.map((item) => {
						switch (item.kind) {
							case "notice":
								return <NoticeLine key={item.seq}>{item.text}</NoticeLine>;
							case "brief":
								return (
									<section key={item.seq} className="rounded-lg border bg-muted/40 p-4">
										<p className="mb-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
											Brief from {item.participant}
										</p>
										<BubbleMessage said={{ ...item, kind: "said", agent: false, participant: "brief" }} />
									</section>
								);
							case "said":
								return <BubbleMessage key={item.seq} said={item} />;
							case "activity":
								return (
									<div key={item.firstSeq} className="flex min-w-0 flex-col gap-2">
										{item.entries.map((entry) => (
											<NarratedView key={entry.seq} entry={entry} />
										))}
										{!item.closed &&
											RUNNING.map((tool) => <RunningToolView key={tool.callId} tool={tool} />)}
									</div>
								);
						}
					})}
				</ConversationContent>
				<ConversationScrollButton />
			</Conversation>
			<div className="shrink-0 px-3 pt-3">
				<HeldQueue />
			</div>
			<KestrelComposer />
		</>
	);
}

function StepsActivity({ activity }: { activity: ActivityGroup }) {
	const live = !activity.closed;
	const duration = workedFor(activity);
	return (
		<ChainOfThought defaultOpen={live} className="max-w-none">
			<ChainOfThoughtHeader>
				{live ? (
					<Shimmer duration={1.5}>Working</Shimmer>
				) : (
					`Worked for ${duration}`
				)}
				<span className="text-muted-foreground"> · {activityLine(activity)}</span>
			</ChainOfThoughtHeader>
			<ChainOfThoughtContent>
				{activity.entries.map((entry) => (
					<Step key={entry.seq} entry={entry} />
				))}
				{live &&
					RUNNING.map((tool) => (
						<ChainOfThoughtStep
							key={tool.callId}
							status="active"
							icon={ChevronRightIcon}
							label={<RunningLabel tool={tool} />}
						/>
					))}
			</ChainOfThoughtContent>
		</ChainOfThought>
	);
}

function RunningLabel({ tool }: { tool: (typeof RUNNING)[number] }) {
	const since = useTicking(Date.parse(tool.startedAt));
	return (
		<span className="flex flex-wrap items-baseline gap-2">
			<Shimmer duration={2} className="font-mono text-[0.8125rem]">
				{tool.title}
			</Shimmer>
			<span className="text-muted-foreground text-xs tabular-nums">running · {since}</span>
		</span>
	);
}

function Step({ entry }: { entry: Narrated }) {
	switch (entry.kind) {
		case "thought":
			return (
				<ChainOfThoughtStep
					icon={BrainIcon}
					label={<span className="line-clamp-2">{entry.text.split("\n")[0]}</span>}
				/>
			);
		case "plan":
			return (
				<ChainOfThoughtStep icon={ListTodoIcon} label="Plan">
					<PlanView plan={entry} />
				</ChainOfThoughtStep>
			);
		case "tool": {
			const failed = entry.status === "failed";
			return (
				<ChainOfThoughtStep
					icon={failed ? XCircleIcon : undefined}
					className={cn(failed && "text-destructive")}
					label={<ToolTitle call={entry} />}
				>
					<ToolCallView call={entry} />
				</ChainOfThoughtStep>
			);
		}
	}
}

export function VariantB() {
	return (
		<>
			<header className="grid shrink-0 gap-1 border-b px-4 py-3">
				<SessionTitle />
				<SessionMeta />
			</header>
			<Conversation>
				<ConversationContent className="gap-5">
					{FLOW.map((item) => {
						switch (item.kind) {
							case "notice":
								return <NoticeLine key={item.seq}>{item.text}</NoticeLine>;
							case "brief":
								return (
									<div key={item.seq} className="grid gap-1">
										<LogMessage said={{ ...item, kind: "said", agent: false }} />
										<p className="ml-10 text-muted-foreground text-xs">Brief</p>
									</div>
								);
							case "said":
								return <LogMessage key={item.seq} said={item} />;
							case "activity":
								return (
									<div key={item.firstSeq} className="ml-10 min-w-0">
										<StepsActivity activity={item} />
									</div>
								);
						}
					})}
				</ConversationContent>
				<ConversationScrollButton />
			</Conversation>
			<KestrelComposer
				header={<HeldQueue className="border-0 p-0" />}
				tools={
					<>
						<ModelPicker compact />
						<UsageContext />
					</>
				}
				suggestions
			/>
		</>
	);
}

function FoldedActivity({ activity }: { activity: ActivityGroup }) {
	const [open, setOpen] = useState(false);
	const live = !activity.closed;
	const running = RUNNING[0];
	return (
		<Collapsible open={open} onOpenChange={setOpen} className="group/activity">
			<CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-muted-foreground text-sm hover:bg-accent hover:text-foreground">
				<ChevronRightIcon
					aria-hidden
					className={cn("size-4 shrink-0 transition-transform", open && "rotate-90")}
				/>
				{live ? (
					<span className="flex min-w-0 items-baseline gap-2">
						<Shimmer duration={1.5}>Working</Shimmer>
						{running && (
							<span className="truncate font-mono text-xs">{running.title}</span>
						)}
					</span>
				) : (
					<span>
						Worked for {workedFor(activity)}
						<span className="text-muted-foreground"> · {activityLine(activity)}</span>
					</span>
				)}
			</CollapsibleTrigger>
			<CollapsibleContent>
				<div className="mt-2 ml-4 flex min-w-0 flex-col gap-2 border-l pl-4">
					{activity.entries.map((entry) => (
						<NarratedView key={entry.seq} entry={entry} />
					))}
					{live && RUNNING.map((tool) => <RunningToolView key={tool.callId} tool={tool} />)}
				</div>
			</CollapsibleContent>
		</Collapsible>
	);
}

export function VariantC() {
	return (
		<>
			<header className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b px-4 py-2">
				<div className="grid min-w-0 gap-0.5">
					<SessionTitle />
					<SessionMeta />
				</div>
				<div className="flex items-center gap-1">
					<ModelPicker compact />
					<UsageContext />
				</div>
			</header>
			<Conversation>
				<ConversationContent className="gap-4">
					{FLOW.map((item) => {
						switch (item.kind) {
							case "notice":
								return <NoticeLine key={item.seq}>{item.text}</NoticeLine>;
							case "brief":
								return (
									<blockquote key={item.seq} className="border-l-2 pl-4">
										<p className="mb-1 font-medium text-muted-foreground text-xs">
											Brief · {item.participant}
										</p>
										<p className="text-pretty text-sm">{item.text}</p>
									</blockquote>
								);
							case "said":
								return <LogMessage key={item.seq} said={item} />;
							case "activity":
								return <FoldedActivity key={item.firstSeq} activity={item} />;
						}
					})}
				</ConversationContent>
				<ConversationScrollButton />
			</Conversation>
			<div className="shrink-0 px-3 pt-3">
				<HeldQueue />
			</div>
			<KestrelComposer />
		</>
	);
}

export const VARIANTS = {
	A: { name: "Cards", Pane: VariantA },
	B: { name: "Steps", Pane: VariantB },
	C: { name: "Folded", Pane: VariantC },
	D: { name: "Steps, refined", Pane: VariantD },
	E: { name: "Aspirational", Pane: MainE },
} as const;
