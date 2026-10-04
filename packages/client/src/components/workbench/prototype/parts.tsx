// PROTOTYPE (#492): one AI Elements composition per kestrel concept, shared by the variants.
import {
	CheckIcon,
	FileTextIcon,
	GitCommitHorizontalIcon,
	ListTodoIcon,
	PencilIcon,
	SearchIcon,
	SquareTerminalIcon,
	Trash2Icon,
	WrenchIcon,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import type { BundledLanguage } from "shiki";
import {
	CodeBlock,
	CodeBlockActions,
	CodeBlockCopyButton,
	CodeBlockFilename,
	CodeBlockHeader,
	CodeBlockTitle,
} from "#/components/ai-elements/code-block";
import {
	Commit,
	CommitAuthor,
	CommitAuthorAvatar,
	CommitContent,
	CommitFile,
	CommitFileAdditions,
	CommitFileChanges,
	CommitFileDeletions,
	CommitFileIcon,
	CommitFileInfo,
	CommitFilePath,
	CommitFileStatus,
	CommitFiles,
	CommitHash,
	CommitHeader,
	CommitInfo,
	CommitMessage,
	CommitMetadata,
	CommitSeparator,
} from "#/components/ai-elements/commit";
import {
	Context,
	ContextContent,
	ContextContentFooter,
	ContextContentHeader,
	ContextTrigger,
} from "#/components/ai-elements/context";
import { FileTree, FileTreeFile, FileTreeFolder } from "#/components/ai-elements/file-tree";
import { Message, MessageContent, MessageResponse } from "#/components/ai-elements/message";
import {
	ModelSelector,
	ModelSelectorContent,
	ModelSelectorEmpty,
	ModelSelectorGroup,
	ModelSelectorInput,
	ModelSelectorItem,
	ModelSelectorList,
	ModelSelectorLogo,
	ModelSelectorName,
	ModelSelectorTrigger,
} from "#/components/ai-elements/model-selector";
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
import {
	Queue,
	QueueItem,
	QueueItemAction,
	QueueItemActions,
	QueueItemContent,
	QueueItemDescription,
	QueueItemIndicator,
	QueueList,
	QueueSection,
	QueueSectionContent,
	QueueSectionLabel,
	QueueSectionTrigger,
} from "#/components/ai-elements/queue";
import { Reasoning, ReasoningContent, ReasoningTrigger } from "#/components/ai-elements/reasoning";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Suggestion, Suggestions } from "#/components/ai-elements/suggestion";
import { Terminal } from "#/components/ai-elements/terminal";
import { Tool, ToolContent, ToolHeader, ToolInput, ToolOutput } from "#/components/ai-elements/tool";
import { Button } from "#/components/ui/button";
import { cn } from "#/lib/utils";
import { elapsed } from "#/operator/transcript-view";
import {
	COMMANDS,
	COMMITS,
	FILES,
	HELD,
	ME,
	MODEL_GROUPS,
	NOW,
	type Plan,
	QUEUED_SESSIONS,
	type RunningTool,
	type Said,
	SESSION,
	type Thought,
	type ToolCall,
	UNPUBLISHED,
	USAGE,
	WORKSPACES,
} from "./fixtures";

const KIND_ICON: Record<string, typeof WrenchIcon> = {
	read: FileTextIcon,
	edit: PencilIcon,
	search: SearchIcon,
	execute: SquareTerminalIcon,
};

function kindIcon(kind: string) {
	const Icon = KIND_ICON[kind] ?? WrenchIcon;
	return <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden />;
}

function Mono({ children }: { children: ReactNode }) {
	return <span className="font-mono text-[0.8125rem]">{children}</span>;
}

export function ToolTitle({ call }: { call: { title: string; toolKind: string } }) {
	return call.toolKind === "execute" ? <Mono>{call.title}</Mono> : call.title;
}

// A completed call, read from the Activity's entries.
export function ToolCallView({ call, defaultOpen }: { call: ToolCall; defaultOpen?: boolean }) {
	const duration = elapsed(Date.parse(call.finishedAt) - Date.parse(call.startedAt));
	return (
		<Tool defaultOpen={defaultOpen} className="mb-0">
			<ToolHeader
				type="dynamic-tool"
				toolName={call.toolKind}
				title={<ToolTitle call={call} />}
				icon={kindIcon(call.toolKind)}
				state={call.status === "failed" ? "output-error" : "output-available"}
				meta={
					<span className="shrink-0 text-muted-foreground text-xs tabular-nums">
						{duration}
						{call.exit !== undefined && ` · exit ${call.exit}`}
					</span>
				}
			/>
			<ToolContent>
				{call.toolKind === "execute" ? (
					<Terminal output={String(call.result)} className="max-h-72" />
				) : (
					<>
						<ToolInput input={call.input} />
						{call.status === "failed" ? (
							<ToolOutput output={undefined} errorText={String(call.result)} />
						) : (
							<ToolOutput
								output={
									<CodeBlock
										code={String(call.result)}
										language={(call.language ?? "text") as BundledLanguage}
									/>
								}
								errorText={undefined}
							/>
						)}
					</>
				)}
			</ToolContent>
		</Tool>
	);
}

// A running call, from session_state: a title, a kind, a status and a start. Nothing to open.
export function RunningToolView({ tool }: { tool: RunningTool }) {
	const since = useTicking(Date.parse(tool.startedAt));
	return (
		<Tool className="mb-0" disabled>
			<ToolHeader
				type="dynamic-tool"
				toolName={tool.toolKind}
				title={<ToolTitle call={tool} />}
				icon={kindIcon(tool.toolKind)}
				state="input-available"
				meta={<span className="shrink-0 text-muted-foreground text-xs tabular-nums">{since}</span>}
				className="[&>svg:last-child]:invisible"
			/>
		</Tool>
	);
}

export function useTicking(from: number): string | undefined {
	const [offset, setOffset] = useState(0);
	useEffect(() => {
		const timer = setInterval(() => setOffset((value) => value + 1000), 1000);
		return () => clearInterval(timer);
	}, []);
	return elapsed(NOW + offset - from);
}

export function ThoughtView({ thought }: { thought: Thought }) {
	return (
		<Reasoning duration={thought.seconds} defaultOpen={false} className="mb-0">
			<ReasoningTrigger />
			<ReasoningContent>{thought.text}</ReasoningContent>
		</Reasoning>
	);
}

export function LiveThinking() {
	return (
		<div className="flex items-center gap-2 text-muted-foreground text-sm">
			<Shimmer duration={1.5}>opencode is working…</Shimmer>
		</div>
	);
}

export function PlanView({ plan }: { plan: Plan }) {
	return (
		<Queue className="shadow-none">
			<QueueSection>
				<QueueSectionTrigger>
					<QueueSectionLabel
						label="plan steps"
						count={plan.steps.length}
						icon={<ListTodoIcon className="size-4" />}
					/>
				</QueueSectionTrigger>
				<QueueSectionContent>
					<QueueList>
						{plan.steps.map((step) => (
							<QueueItem key={step.content}>
								<div className="flex items-center gap-2">
									<QueueItemIndicator completed={step.status === "completed"} />
									<QueueItemContent
										completed={step.status === "completed"}
										className={cn(step.status === "in_progress" && "text-foreground")}
									>
										{step.content}
									</QueueItemContent>
								</div>
							</QueueItem>
						))}
					</QueueList>
				</QueueSectionContent>
			</QueueSection>
		</Queue>
	);
}

function initials(name: string) {
	return name.slice(0, 2).toUpperCase();
}

export function at(timestamp: string) {
	return new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// Bubbles: the local Participant's own messages sit on the right, as AI Elements ships them.
export function BubbleMessage({ said }: { said: Said }) {
	const mine = said.participant === ME;
	return (
		<Message from={mine ? "user" : "assistant"} className="max-w-full">
			{!mine && (
				<span className="flex items-center gap-2 text-muted-foreground text-xs">
					<span className="font-medium text-foreground">{said.participant}</span>
					{at(said.at)}
				</span>
			)}
			<MessageContent
				className={cn(!mine && !said.agent && "rounded-lg border bg-background px-4 py-3")}
			>
				<MessageResponse>{said.text}</MessageResponse>
			</MessageContent>
		</Message>
	);
}

// Log: everyone flush left with an avatar and a name, because a Transcript has many Participants.
export function LogMessage({ said }: { said: Said }) {
	return (
		<article className="flex gap-3">
			<span
				aria-hidden
				className={cn(
					"mt-0.5 grid size-7 shrink-0 place-items-center rounded-full font-medium text-[0.6875rem]",
					said.agent ? "bg-foreground text-background" : "bg-muted text-foreground",
				)}
			>
				{initials(said.participant)}
			</span>
			<Message from="assistant" className="min-w-0 max-w-full gap-1">
				<span className="flex items-baseline gap-2 text-xs">
					<span className="font-semibold text-sm">{said.participant}</span>
					<span className="text-muted-foreground">{at(said.at)}</span>
				</span>
				<MessageContent>
					<MessageResponse>{said.text}</MessageResponse>
				</MessageContent>
			</Message>
		</article>
	);
}

export function NoticeLine({ children }: { children: ReactNode }) {
	return (
		<p className="flex items-center gap-3 text-muted-foreground text-xs">
			<span className="h-px flex-1 bg-border" />
			{children}
			<span className="h-px flex-1 bg-border" />
		</p>
	);
}

export function ModelPicker({ compact }: { compact?: boolean }) {
	const [open, setOpen] = useState(false);
	const [model, setModel] = useState(SESSION.model);
	const chosen = MODEL_GROUPS.flatMap((group) => group.models).find((one) => one.id === model);
	return (
		<ModelSelector open={open} onOpenChange={setOpen}>
			<ModelSelectorTrigger
				render={
					<Button variant={compact ? "ghost" : "outline"} size="sm" className="gap-2">
						{chosen && <ModelSelectorLogo provider={chosen.provider} />}
						<ModelSelectorName>{chosen?.name ?? "Default model"}</ModelSelectorName>
						{chosen && !compact && (
							<span className="text-muted-foreground">
								{MODEL_GROUPS.find((group) => group.provider === chosen.provider)?.label}
							</span>
						)}
					</Button>
				}
			/>
			<ModelSelectorContent title="Choose a model">
				<ModelSelectorInput placeholder="Search models…" />
				<ModelSelectorList>
					<ModelSelectorEmpty>No model matches.</ModelSelectorEmpty>
					{MODEL_GROUPS.map((group) => (
						<ModelSelectorGroup key={group.provider} heading={group.label}>
							{group.models.map((one) => (
								<ModelSelectorItem
									key={one.id}
									value={one.id}
									keywords={[one.name, group.label]}
									onSelect={() => {
										setModel(one.id);
										setOpen(false);
									}}
								>
									<ModelSelectorLogo provider={one.provider} />
									<ModelSelectorName>{one.name}</ModelSelectorName>
									<span className="ml-auto font-mono text-muted-foreground text-xs">{one.id}</span>
									{one.id === model && <CheckIcon className="size-4" />}
								</ModelSelectorItem>
							))}
						</ModelSelectorGroup>
					))}
				</ModelSelectorList>
			</ModelSelectorContent>
		</ModelSelector>
	);
}

export function UsageContext() {
	return (
		<Context usedTokens={USAGE.context_used} maxTokens={USAGE.context_size}>
			<ContextTrigger size="sm" />
			<ContextContent>
				<ContextContentHeader />
				<ContextContentFooter>
					<span className="text-muted-foreground">Spent this Session</span>
					<span className="tabular-nums">
						{USAGE.cost?.amount.toFixed(2)} {USAGE.cost?.currency}
					</span>
				</ContextContentFooter>
			</ContextContent>
		</Context>
	);
}

export function HeldQueue({ className }: { className?: string }) {
	const [held, setHeld] = useState(HELD);
	if (held.length === 0) return null;
	return (
		<Queue className={cn("shadow-none", className)}>
			<QueueSection>
				<QueueSectionTrigger>
					<QueueSectionLabel label={held.length === 1 ? "held message" : "held messages"} count={held.length} />
					<span className="font-normal text-xs">for the next Turn</span>
				</QueueSectionTrigger>
				<QueueSectionContent>
					<QueueList>
						{held.map((message) => (
							<QueueItem key={message.id}>
								<div className="flex items-start gap-2">
									<QueueItemIndicator />
									<QueueItemContent className="line-clamp-2 text-foreground">{message.message}</QueueItemContent>
									{message.participant === ME && (
										<QueueItemActions>
											<QueueItemAction aria-label="Edit the held message">
												<PencilIcon className="size-3" />
											</QueueItemAction>
											<QueueItemAction
												aria-label="Withdraw the held message"
												onClick={() => setHeld((all) => all.filter((one) => one.id !== message.id))}
											>
												<Trash2Icon className="size-3" />
											</QueueItemAction>
										</QueueItemActions>
									)}
								</div>
								<QueueItemDescription>
									{message.participant} · {at(message.posted_at)}
									{message.edited_at && " · edited"}
								</QueueItemDescription>
							</QueueItem>
						))}
					</QueueList>
				</QueueSectionContent>
			</QueueSection>
		</Queue>
	);
}

function CommandMenu() {
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
							<Mono>/{command.name}</Mono>
							{command.input_hint && (
								<span className="font-mono text-muted-foreground text-xs">{command.input_hint}</span>
							)}
							<span className="ml-auto truncate text-muted-foreground text-xs">{command.description}</span>
						</button>
					</li>
				))}
			</ul>
		</PromptInputHeader>
	);
}

export function KestrelComposer({
	header,
	tools,
	suggestions,
}: {
	header?: ReactNode;
	tools?: ReactNode;
	suggestions?: boolean;
}) {
	return (
		<PromptInputProvider>
			<div className="grid shrink-0 gap-2 border-t p-3">
				{suggestions && <CommandSuggestions />}
				<PromptInput onSubmit={(_, event) => event.preventDefault()}>
					{header && <PromptInputHeader className="block p-2 pb-0">{header}</PromptInputHeader>}
					<CommandMenu />
					<PromptInputBody>
						<PromptInputTextarea placeholder="Hold for the next Turn — type / for commands" />
					</PromptInputBody>
					<PromptInputFooter>
						<PromptInputTools>
							{tools}
							<PromptInputButton variant="ghost">Interrupt</PromptInputButton>
						</PromptInputTools>
						<PromptInputSubmit status="streaming" />
					</PromptInputFooter>
				</PromptInput>
				<p className="text-muted-foreground text-xs">
					writing as <span className="font-medium text-foreground">{ME}</span> · a Turn is working, so
					a message is held
				</p>
			</div>
		</PromptInputProvider>
	);
}

function CommandSuggestions() {
	const { textInput } = usePromptInputController();
	return (
		<Suggestions>
			{COMMANDS.map((command) => (
				<Suggestion
					key={command.name}
					suggestion={`/${command.name}`}
					onClick={(value) => textInput.setInput(`${value} `)}
					className="font-mono"
				/>
			))}
		</Suggestions>
	);
}

export function DiffView() {
	return (
		<div className="grid gap-4 p-4">
			<section className="grid gap-2">
				<h3 className="font-semibold text-sm">Unpublished changes</h3>
				<CodeBlock code={UNPUBLISHED.diff} language="diff">
					<CodeBlockHeader>
						<CodeBlockTitle>
							<CodeBlockFilename>{UNPUBLISHED.repository}</CodeBlockFilename>
							<span className="text-muted-foreground text-xs">
								{UNPUBLISHED.files.length} files · +18 −2
							</span>
						</CodeBlockTitle>
						<CodeBlockActions>
							<CodeBlockCopyButton />
						</CodeBlockActions>
					</CodeBlockHeader>
				</CodeBlock>
			</section>
			<section className="grid gap-2">
				<h3 className="font-semibold text-sm">Commits</h3>
				{COMMITS.map((commit) => (
					<Commit key={commit.hash}>
						<CommitHeader>
							<CommitAuthor>
								<CommitAuthorAvatar initials="OC" className="size-7" />
							</CommitAuthor>
							<CommitInfo>
								<CommitMessage>{commit.message}</CommitMessage>
								<CommitMetadata className="flex-wrap">
									<CommitHash>{commit.hash.slice(0, 7)}</CommitHash>
									<CommitSeparator />
									<span>{at(commit.at)}</span>
								</CommitMetadata>
							</CommitInfo>
						</CommitHeader>
						<CommitContent>
							<CommitFiles>
								{commit.files.map((file) => (
									<CommitFile key={file.path}>
										<CommitFileInfo>
											<CommitFileStatus status={file.status} />
											<CommitFileIcon />
											<CommitFilePath>{file.path}</CommitFilePath>
										</CommitFileInfo>
										<CommitFileChanges>
											<CommitFileAdditions count={file.added} />
											<CommitFileDeletions count={file.removed} />
										</CommitFileChanges>
									</CommitFile>
								))}
							</CommitFiles>
						</CommitContent>
					</Commit>
				))}
			</section>
		</div>
	);
}

export function FilesView() {
	const [selected, setSelected] = useState("crates/kestrel/src/follow.rs");
	return (
		<div className="grid gap-3 p-4">
			<FileTree
				defaultExpanded={new Set(["crates", "crates/kestrel", "crates/kestrel/src"])}
				selectedPath={selected}
				onSelect={setSelected}
			>
				<FileTreeFolder path="crates" name="crates">
					<FileTreeFolder path="crates/kestrel" name="kestrel">
						<FileTreeFolder path="crates/kestrel/src" name="src">
							<FileTreeFile path="crates/kestrel/src/follow.rs" name="follow.rs" />
							<FileTreeFile path="crates/kestrel/src/lib.rs" name="lib.rs" />
							<FileTreeFolder path="crates/kestrel/src/session" name="session">
								<FileTreeFile path="crates/kestrel/src/session/mod.rs" name="mod.rs" />
								<FileTreeFile path="crates/kestrel/src/session/usage.rs" name="usage.rs" />
							</FileTreeFolder>
						</FileTreeFolder>
						<FileTreeFolder path="crates/kestrel/tests" name="tests">
							<FileTreeFile path="crates/kestrel/tests/usage.rs" name="usage.rs" />
						</FileTreeFolder>
					</FileTreeFolder>
				</FileTreeFolder>
				<FileTreeFile path="Cargo.toml" name="Cargo.toml" />
				<FileTreeFile path="README.md" name="README.md" />
			</FileTree>
			{FILES[selected] !== undefined && (
				<CodeBlock
					code={FILES[selected]}
					language={languageOf(selected)}
					showLineNumbers
				>
					<CodeBlockHeader>
						<CodeBlockTitle>
							<CodeBlockFilename>{selected}</CodeBlockFilename>
						</CodeBlockTitle>
						<CodeBlockActions>
							<CodeBlockCopyButton />
						</CodeBlockActions>
					</CodeBlockHeader>
				</CodeBlock>
			)}
		</div>
	);
}

function languageOf(path: string): BundledLanguage {
	if (path.endsWith(".rs")) return "rust";
	if (path.endsWith(".toml")) return "toml";
	if (path.endsWith(".md")) return "markdown";
	return "text" as BundledLanguage;
}

export function SessionsQueue() {
	return (
		<div className="p-4">
			<Queue className="shadow-none">
				<QueueSection>
					<QueueSectionTrigger>
						<QueueSectionLabel label="queued Sessions" count={QUEUED_SESSIONS.length} />
						<span className="font-normal text-xs">waiting for a slot</span>
					</QueueSectionTrigger>
					<QueueSectionContent>
						<QueueList>
							{QUEUED_SESSIONS.map((queued) => (
								<QueueItem key={queued.workspace}>
									<div className="flex items-center gap-2">
										<QueueItemIndicator />
										<QueueItemContent className="text-foreground">{queued.brief}</QueueItemContent>
										<span className="text-muted-foreground text-xs tabular-nums">#{queued.position}</span>
									</div>
									<QueueItemDescription>
										{queued.workspace} · {queued.participant}
									</QueueItemDescription>
								</QueueItem>
							))}
						</QueueList>
					</QueueSectionContent>
				</QueueSection>
			</Queue>
		</div>
	);
}

export function WorkspacesList() {
	return (
		<nav aria-label="Workspaces" className="grid gap-0.5 p-2">
			{WORKSPACES.map((workspace) => (
				<a
					key={workspace.name}
					href="#"
					aria-current={workspace.current ? "page" : undefined}
					className="grid gap-0.5 rounded-md px-3 py-2 hover:bg-accent aria-[current=page]:bg-accent"
				>
					<span className="flex items-baseline justify-between gap-2">
						<span className="truncate font-medium text-sm">{workspace.name}</span>
						<span className="shrink-0 text-muted-foreground text-xs">{workspace.phase}</span>
					</span>
					<span className="truncate font-mono text-muted-foreground text-xs">
						{workspace.project} · {workspace.branch}
					</span>
				</a>
			))}
		</nav>
	);
}

export function SessionTitle() {
	return (
		<div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
			<h1 className="truncate font-semibold text-base tracking-tight">{SESSION.title}</h1>
			<span className="inline-flex items-center gap-1.5 rounded-full bg-secondary px-2 py-0.5 font-medium text-xs">
				<span className="size-1.5 animate-pulse rounded-full bg-emerald-500" />
				{SESSION.phase}
			</span>
		</div>
	);
}

export function SessionMeta() {
	return (
		<p className="flex flex-wrap items-center gap-x-2 text-muted-foreground text-xs">
			<GitCommitHorizontalIcon className="size-3.5" aria-hidden />
			<span className="font-mono">
				{SESSION.project} · {SESSION.branch}
			</span>
			<span>·</span>
			<span>{SESSION.agent}</span>
			<span>·</span>
			<span>{SESSION.watching}</span>
		</p>
	);
}
