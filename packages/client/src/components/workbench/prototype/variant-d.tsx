// PROTOTYPE (#492): D "Steps, refined": B in a quieter language, after Conductor.
// Activity folds to one line; each call is one line that opens in place; agent prose is unframed;
// people's messages are light bubbles; the reply carries a footer of what it changed.
import {
	ArrowUpIcon,
	BrainIcon,
	ChevronRightIcon,
	CopyIcon,
	FileDiffIcon,
	FileTextIcon,
	ListTodoIcon,
	PencilIcon,
	SearchIcon,
	SquareTerminalIcon,
	WrenchIcon,
	XIcon,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import type { BundledLanguage } from "shiki";
import { CodeBlock } from "#/components/ai-elements/code-block";
import {
	Conversation,
	ConversationContent,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import { MessageResponse } from "#/components/ai-elements/message";
import {
	PromptInput,
	PromptInputBody,
	PromptInputButton,
	PromptInputFooter,
	PromptInputHeader,
	PromptInputProvider,
	PromptInputSubmit,
	PromptInputTextarea,
	PromptInputTools,
	usePromptInputController,
} from "#/components/ai-elements/prompt-input";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Terminal } from "#/components/ai-elements/terminal";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { cn } from "#/lib/utils";
import { elapsed } from "#/operator/transcript-view";
import {
	type ActivityGroup,
	COMMANDS,
	COMMITS,
	FLOW,
	HELD,
	ME,
	type Narrated,
	RUNNING,
	type Said,
	SESSION,
	UNPUBLISHED,
	WORKSPACES,
} from "./fixtures";
import { at, ModelPicker, PlanView, UsageContext, useTicking } from "./parts";

const KIND_ICON: Record<string, typeof WrenchIcon> = {
	read: FileTextIcon,
	edit: PencilIcon,
	search: SearchIcon,
	execute: SquareTerminalIcon,
};

function KindIcon({ kind, className }: { kind: string; className?: string }) {
	const Icon = KIND_ICON[kind] ?? WrenchIcon;
	return <Icon aria-hidden className={cn("size-3.5 shrink-0", className)} />;
}

function plural(count: number, one: string) {
	return `${count} ${count === 1 ? one : `${one}s`}`;
}

function summary(activity: ActivityGroup) {
	const { tools, failed, thoughts, plans } = activity.counts;
	return [
		tools && plural(tools, "tool call"),
		failed && `${failed} failed`,
		thoughts && plural(thoughts, "thought"),
		plans && plural(plans, "plan"),
	]
		.filter(Boolean)
		.join(", ");
}

function kindsIn(activity: ActivityGroup) {
	return [...new Set(activity.entries.flatMap((entry) => (entry.kind === "tool" ? [entry.toolKind] : [])))];
}

function Row({
	icon,
	label,
	trailing,
	tone,
	children,
}: {
	icon: ReactNode;
	label: ReactNode;
	trailing?: ReactNode;
	tone?: "failed";
	children?: ReactNode;
}) {
	const [open, setOpen] = useState(false);
	return (
		<Collapsible open={open} onOpenChange={setOpen}>
			<CollapsibleTrigger
				disabled={!children}
				className={cn(
					"flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left text-[0.8125rem] text-muted-foreground",
					children && "hover:bg-accent hover:text-foreground",
					tone === "failed" && "text-destructive hover:text-destructive",
				)}
			>
				{icon}
				<span className="min-w-0 flex-1 truncate">{label}</span>
				{trailing && <span className="shrink-0 text-xs tabular-nums">{trailing}</span>}
			</CollapsibleTrigger>
			{children && (
				<CollapsibleContent>
					<div className="my-1 ml-6 min-w-0">{children}</div>
				</CollapsibleContent>
			)}
		</Collapsible>
	);
}

function Entry({ entry }: { entry: Narrated }) {
	switch (entry.kind) {
		case "thought":
			return (
				<Row
					icon={<BrainIcon aria-hidden className="size-3.5 shrink-0" />}
					label={<span className="italic">{entry.text.split("\n")[0]?.replace(/[`*]/g, "")}</span>}
					trailing={`${entry.seconds}s`}
				>
					<div className="text-muted-foreground text-sm">
						<MessageResponse>{entry.text}</MessageResponse>
					</div>
				</Row>
			);
		case "plan":
			return (
				<Row icon={<ListTodoIcon aria-hidden className="size-3.5 shrink-0" />} label="Updated the plan">
					<PlanView plan={entry} />
				</Row>
			);
		case "tool": {
			const failed = entry.status === "failed";
			const duration = elapsed(Date.parse(entry.finishedAt) - Date.parse(entry.startedAt));
			return (
				<Row
					icon={failed ? <XIcon aria-hidden className="size-3.5 shrink-0" /> : <KindIcon kind={entry.toolKind} />}
					label={entry.toolKind === "execute" ? <span className="font-mono text-xs">{entry.title}</span> : entry.title}
					trailing={[entry.exit !== undefined && `exit ${entry.exit}`, failed ? "failed" : duration].filter(Boolean).join(" · ")}
					tone={failed ? "failed" : undefined}
				>
					{entry.toolKind === "execute" ? (
						<Terminal output={String(entry.result)} className="max-h-64 text-xs" />
					) : failed ? (
						<p className="font-mono text-destructive text-xs">{String(entry.result)}</p>
					) : (
						<CodeBlock code={String(entry.result)} language={(entry.language ?? "text") as BundledLanguage} />
					)}
				</Row>
			);
		}
	}
}

function RunningRow({ tool }: { tool: (typeof RUNNING)[number] }) {
	const since = useTicking(Date.parse(tool.startedAt));
	return (
		<Row
			icon={<KindIcon kind={tool.toolKind} className="text-foreground" />}
			label={
				<Shimmer duration={2} className="font-mono text-xs">
					{tool.title}
				</Shimmer>
			}
			trailing={since}
		/>
	);
}

function ActivityView({ activity }: { activity: ActivityGroup }) {
	const live = !activity.closed;
	const [open, setOpen] = useState(live);
	const duration =
		activity.finishedAt && elapsed(Date.parse(activity.finishedAt) - Date.parse(activity.startedAt));
	return (
		<Collapsible open={open} onOpenChange={setOpen}>
			<CollapsibleTrigger className="flex items-center gap-1.5 rounded-md py-0.5 pr-1.5 text-muted-foreground text-sm hover:text-foreground">
				<ChevronRightIcon aria-hidden className={cn("size-3.5 transition-transform", open && "rotate-90")} />
				{live ? <Shimmer duration={1.5}>Working</Shimmer> : <span>{summary(activity)}</span>}
				{live && <span>· {summary(activity)}</span>}
				<span className="flex items-center gap-1 pl-1 opacity-70">
					{kindsIn(activity).map((kind) => (
						<KindIcon key={kind} kind={kind} />
					))}
				</span>
				{duration && <span className="text-xs tabular-nums opacity-70">{duration}</span>}
			</CollapsibleTrigger>
			<CollapsibleContent>
				<div className="mt-1 ml-[0.4375rem] grid min-w-0 grid-cols-[minmax(0,1fr)] gap-px border-l pl-3">
					{activity.entries.map((entry) => (
						<Entry key={entry.seq} entry={entry} />
					))}
					{live && RUNNING.map((tool) => <RunningRow key={tool.callId} tool={tool} />)}
				</div>
			</CollapsibleContent>
		</Collapsible>
	);
}

function Byline({ name, time, agent }: { name: string; time: string; agent?: boolean }) {
	return (
		<p className="mb-1 flex items-baseline gap-2 text-xs">
			<span className={cn("font-medium", agent ? "text-muted-foreground" : "text-foreground")}>{name}</span>
			<span className="text-muted-foreground">{at(time)}</span>
		</p>
	);
}

function PersonMessage({ said }: { said: Pick<Said, "participant" | "text" | "at"> }) {
	return (
		<div>
			<Byline name={said.participant === ME ? "you" : said.participant} time={said.at} />
			<div className="w-fit max-w-[85%] rounded-lg bg-muted px-3 py-2 text-sm">
				<MessageResponse>{said.text}</MessageResponse>
			</div>
		</div>
	);
}

function AgentMessage({ said, changed }: { said: Said; changed?: { path: string; added: number; removed: number }[] }) {
	return (
		<div>
			<Byline name={said.participant} time={said.at} agent />
			<div className="text-sm leading-relaxed">
				<MessageResponse>{said.text}</MessageResponse>
			</div>
			<div className="mt-2 flex flex-wrap items-center gap-1.5 text-muted-foreground text-xs">
				<span className="tabular-nums">2m 14s</span>
				<button type="button" aria-label="Copy the message" className="rounded p-1 hover:bg-accent hover:text-foreground">
					<CopyIcon className="size-3.5" />
				</button>
				{changed?.map((file) => (
					<span key={file.path} className="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5">
						<FileDiffIcon aria-hidden className="size-3" />
						<span className="font-mono">{file.path.split("/").at(-1)}</span>
						<span className="text-emerald-600 dark:text-emerald-400">+{file.added}</span>
						<span className="text-red-600 dark:text-red-400">−{file.removed}</span>
					</span>
				))}
			</div>
		</div>
	);
}

function Header() {
	return (
		<header className="flex shrink-0 items-center gap-3 border-b px-4 py-2.5">
			<div className="min-w-0 flex-1">
				<div className="flex items-center gap-2">
					<h1 className="truncate font-semibold text-sm">{SESSION.title}</h1>
					<span className="inline-flex shrink-0 items-center gap-1.5 text-muted-foreground text-xs">
						<span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
						{SESSION.phase}
					</span>
				</div>
				<p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
					{SESSION.project} · {SESSION.branch}
				</p>
			</div>
			<span className="flex shrink-0 -space-x-1.5" aria-label="Watching: sam and 1 anonymous">
				{["SA", "+1"].map((who) => (
					<span
						key={who}
						className="grid size-6 place-items-center rounded-full border-2 border-background bg-muted font-medium text-[0.625rem]"
					>
						{who}
					</span>
				))}
			</span>
		</header>
	);
}

function HeldLine() {
	const [held, setHeld] = useState(HELD);
	if (held.length === 0) return null;
	return (
		<PromptInputHeader className="block p-2 pb-0">
			<p className="px-1 pb-1 text-muted-foreground text-xs">Held for the next Turn</p>
			<ul className="grid gap-1">
				{held.map((message) => (
					<li key={message.id} className="group flex items-start gap-2 rounded-md bg-muted/60 px-2 py-1.5 text-xs">
						<span className="shrink-0 font-medium">{message.participant === ME ? "you" : message.participant}</span>
						<span className="min-w-0 flex-1 text-muted-foreground">{message.message}</span>
						{message.participant === ME && (
							<button
								type="button"
								aria-label="Withdraw the held message"
								onClick={() => setHeld((all) => all.filter((one) => one.id !== message.id))}
								className="rounded p-0.5 text-muted-foreground hover:text-foreground"
							>
								<XIcon className="size-3" />
							</button>
						)}
					</li>
				))}
			</ul>
		</PromptInputHeader>
	);
}

function Commands() {
	const { textInput } = usePromptInputController();
	const typed = textInput.value;
	if (!typed.startsWith("/") || typed.includes(" ")) return null;
	const matches = COMMANDS.filter((command) => command.name.startsWith(typed.slice(1)));
	if (matches.length === 0) return null;
	return (
		<PromptInputHeader className="block p-1">
			<ul aria-label="Commands" className="grid">
				{matches.map((command) => (
					<li key={command.name}>
						<button
							type="button"
							className="flex w-full items-baseline gap-3 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent"
							onClick={() => textInput.setInput(`/${command.name} `)}
						>
							<span className="font-mono text-xs">/{command.name}</span>
							<span className="truncate text-muted-foreground text-xs">{command.description}</span>
						</button>
					</li>
				))}
			</ul>
		</PromptInputHeader>
	);
}

function Composer() {
	return (
		<PromptInputProvider>
			<div className="shrink-0 px-3 pb-3">
				<PromptInput onSubmit={(_, event) => event.preventDefault()} className="rounded-xl">
					<HeldLine />
					<Commands />
					<PromptInputBody>
						<PromptInputTextarea placeholder="Message the Session — / for commands" className="min-h-14 text-sm" />
					</PromptInputBody>
					<PromptInputFooter>
						<PromptInputTools>
							<ModelPicker compact />
							<UsageContext />
						</PromptInputTools>
						<span className="flex shrink-0 items-center gap-1">
							<PromptInputButton variant="ghost" size="sm" className="w-auto px-2 text-xs">
								Interrupt
							</PromptInputButton>
							<PromptInputSubmit className="rounded-lg">
								<ArrowUpIcon className="size-4" />
							</PromptInputSubmit>
						</span>
					</PromptInputFooter>
				</PromptInput>
				<p className="mt-1.5 px-1 text-muted-foreground text-xs">
					A Turn is working, so your message is held until it ends
				</p>
			</div>
		</PromptInputProvider>
	);
}

export function VariantD() {
	return (
		<>
			<Header />
			<Conversation>
				<ConversationContent className="mx-auto w-full max-w-3xl gap-5 px-5 py-6">
					{FLOW.map((item) => {
						switch (item.kind) {
							case "notice":
								return (
									<p key={item.seq} className="text-center text-muted-foreground text-xs">
										{item.text}
									</p>
								);
							case "brief":
								return (
									<div key={item.seq}>
										<p className="mb-1 text-muted-foreground text-xs uppercase tracking-wide">Brief</p>
										<PersonMessage said={item} />
									</div>
								);
							case "said":
								return item.agent ? (
									<AgentMessage key={item.seq} said={item} changed={COMMITS[1]?.files} />
								) : (
									<PersonMessage key={item.seq} said={item} />
								);
							case "activity":
								return <ActivityView key={item.firstSeq} activity={item} />;
						}
					})}
				</ConversationContent>
				<ConversationScrollButton />
			</Conversation>
			<Composer />
		</>
	);
}

export function ChangesView() {
	const [open, setOpen] = useState<string | null>(null);
	return (
		<div className="flex min-w-0 flex-col gap-4 py-2">
			<section>
				<p className="flex items-baseline justify-between px-4 pb-1 text-muted-foreground text-xs">
					<span>Unpublished · {UNPUBLISHED.files.length} files</span>
					<span className="tabular-nums">
						<span className="text-emerald-600 dark:text-emerald-400">+18</span>{" "}
						<span className="text-red-600 dark:text-red-400">−2</span>
					</span>
				</p>
				<ul>
					{UNPUBLISHED.files.map((file) => {
						const parts = file.path.split("/");
						const name = parts.pop();
						return (
							<li key={file.path}>
								<button
									type="button"
									aria-expanded={open === file.path}
									onClick={() => setOpen(open === file.path ? null : file.path)}
									className="flex w-full min-w-0 items-baseline gap-1 px-4 py-1.5 text-left text-sm hover:bg-accent"
								>
									<span className="min-w-0 truncate text-muted-foreground [direction:rtl]">
										<bdi>{parts.join("/")}/</bdi>
									</span>
									<span className="shrink-0 font-medium">{name}</span>
									<span className="ml-auto shrink-0 pl-2 text-xs tabular-nums">
										<span className="text-emerald-600 dark:text-emerald-400">+{file.added}</span>{" "}
										<span className="text-red-600 dark:text-red-400">−{file.removed}</span>
									</span>
								</button>
								{open === file.path && (
									<div className="px-2 pb-2">
										<CodeBlock code={UNPUBLISHED.diff} language="diff" className="text-xs" />
									</div>
								)}
							</li>
						);
					})}
				</ul>
			</section>
			<section>
				<p className="px-4 pb-1 text-muted-foreground text-xs">Commits</p>
				<ul>
					{COMMITS.map((commit) => (
						<li key={commit.hash} className="flex items-baseline gap-2 px-4 py-1.5 text-sm">
							<span className="shrink-0 font-mono text-muted-foreground text-xs">{commit.hash.slice(0, 7)}</span>
							<span className="min-w-0 flex-1 truncate">{commit.message}</span>
							<span className="shrink-0 text-muted-foreground text-xs">{at(commit.at)}</span>
						</li>
					))}
				</ul>
			</section>
		</div>
	);
}

const AGO: Record<string, string> = {
	"flaky-usage-test": "now",
	"cli-completions": "22m",
	"compose-up-docs": "1h",
	"relay-spike": "1d",
};

export function WorkspacesD() {
	return (
		<nav aria-label="Workspaces" className="grid gap-px p-2">
			{WORKSPACES.map((workspace) => (
				<a
					key={workspace.name}
					href="#"
					aria-current={workspace.current ? "page" : undefined}
					className="flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm hover:bg-accent aria-[current=page]:bg-accent"
				>
					<span
						aria-hidden
						className={cn(
							"size-1.5 shrink-0 rounded-full",
							workspace.phase === "Working" && "bg-emerald-500",
							workspace.phase === "Waiting for you" && "bg-amber-500",
							workspace.phase === "Queued" && "bg-muted-foreground/50",
							workspace.phase === "Ended" && "bg-transparent",
						)}
					/>
					<span
						className={cn(
							"min-w-0 flex-1 truncate",
							workspace.phase === "Waiting for you" ? "font-semibold" : "text-foreground/80",
						)}
					>
						{workspace.name}
					</span>
					<span className="shrink-0 text-muted-foreground text-xs">{AGO[workspace.name]}</span>
				</a>
			))}
		</nav>
	);
}
