// PROTOTYPE (#492): E "Aspirational": D's language carried past what kestrel has today, through
// 0.5 (Approvals, Questions, Policy, Skills, MCP), 0.6 (Campaigns, enqueue) and 0.7 (Integrations,
// Work Items, Triggers, Outcomes). Every fixture here is invented; nothing is wired.
import {
	ArrowUpIcon,
	AtSignIcon,
	BotIcon,
	CheckIcon,
	ChevronDownIcon,
	ChevronRightIcon,
	CircleDashedIcon,
	ClockIcon,
	FolderIcon,
	GitBranchIcon,
	GitPullRequestIcon,
	HashIcon,
	InboxIcon,
	LayersIcon,
	ListChecksIcon,
	LoaderIcon,
	PanelLeftIcon,
	PanelRightIcon,
	PauseIcon,
	PlusIcon,
	SearchIcon,
	ServerIcon,
	ShieldCheckIcon,
	SparklesIcon,
	SquareIcon,
	TerminalIcon,
	WorkflowIcon,
	XIcon,
	ZapIcon,
} from "lucide-react";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import {
	Confirmation,
	ConfirmationAccepted,
	ConfirmationAction,
	ConfirmationActions,
	ConfirmationRejected,
	ConfirmationRequest,
	ConfirmationTitle,
} from "#/components/ai-elements/confirmation";
import {
	Conversation,
	ConversationContent,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import {
	PromptInput,
	PromptInputBody,
	PromptInputButton,
	PromptInputFooter,
	PromptInputProvider,
	PromptInputSubmit,
	PromptInputTextarea,
	PromptInputTools,
} from "#/components/ai-elements/prompt-input";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Terminal } from "#/components/ai-elements/terminal";
import {
	Command,
	CommandDialog,
	CommandEmpty,
	CommandGroup,
	CommandInput,
	CommandItem,
	CommandList,
	CommandShortcut,
} from "#/components/ui/command";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";
import { cn } from "#/lib/utils";
import { type ActivityGroup, FLOW, type Said } from "./fixtures";
import { ModelPicker, UsageContext, useTicking } from "./parts";
import { ActivityView, AgentMessage, ChangesView, Commands, HeldLine, PersonMessage } from "./variant-d";

export type Collapsed = { workspaces?: boolean; work?: boolean };
export const Sidebars = createContext<{ collapsed: Collapsed; toggle: (side: keyof Collapsed) => void }>({
	collapsed: {},
	toggle: () => {},
});

export function useSidebarKeys(toggle: (side: keyof Collapsed) => void) {
	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key.toLowerCase() !== "b" && event.code !== "KeyB") return;
			if (!(event.metaKey || event.ctrlKey)) return;
			event.preventDefault();
			toggle(event.altKey ? "work" : "workspaces");
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [toggle]);
}

type Source = "github" | "linear" | "slack" | "schedule" | "operator";

function SourceMark({ source, className }: { source: Source; className?: string }) {
	const box = cn(
		"inline-grid size-4 shrink-0 place-items-center rounded-[4px] font-semibold text-[0.5625rem] leading-none",
		className,
	);
	switch (source) {
		case "github":
			return <GitPullRequestIcon aria-label="GitHub" className={cn("size-3.5 shrink-0", className)} />;
		case "linear":
			return (
				<span aria-label="Linear" className={cn(box, "bg-indigo-500/15 text-indigo-600 dark:text-indigo-300")}>
					L
				</span>
			);
		case "slack":
			return <HashIcon aria-label="Slack" className={cn("size-3.5 shrink-0", className)} />;
		case "schedule":
			return <ClockIcon aria-label="Schedule" className={cn("size-3.5 shrink-0", className)} />;
		case "operator":
			return <AtSignIcon aria-label="Opened by hand" className={cn("size-3.5 shrink-0", className)} />;
	}
}

type Phase = "working" | "waiting" | "queued" | "ended" | "failed" | "blocked";

function PhaseDot({ phase }: { phase: Phase }) {
	return (
		<span
			aria-hidden
			className={cn(
				"size-1.5 shrink-0 rounded-full",
				phase === "working" && "animate-pulse bg-emerald-500",
				phase === "waiting" && "bg-amber-500",
				phase === "queued" && "bg-muted-foreground/40",
				phase === "blocked" && "bg-muted-foreground/40 ring-1 ring-muted-foreground/40 ring-offset-1 ring-offset-background",
				phase === "failed" && "bg-red-500",
				phase === "ended" && "bg-transparent ring-1 ring-muted-foreground/40",
			)}
		/>
	);
}

const PROJECTS: {
	name: string;
	open: boolean;
	workspaces: { name: string; phase: Phase; source: Source; ago: string; campaign?: string; current?: boolean }[];
}[] = [
	{
		name: "kestrel",
		open: true,
		workspaces: [
			{ name: "flaky-usage-test", phase: "waiting", source: "linear", ago: "now", campaign: "Stabilize CI", current: true },
			{ name: "cli-follower-fix", phase: "queued", source: "operator", ago: "3m", campaign: "Stabilize CI" },
			{ name: "pr-818-review", phase: "working", source: "github", ago: "6m" },
			{ name: "nightly-deps", phase: "ended", source: "schedule", ago: "9h" },
			{ name: "compose-up-docs", phase: "failed", source: "slack", ago: "1d" },
		],
	},
	{ name: "kestrel-relay", open: false, workspaces: [{ name: "relay-spike", phase: "ended", source: "operator", ago: "2d" }] },
	{ name: "docs-site", open: false, workspaces: [{ name: "search-index", phase: "working", source: "github", ago: "14m" }] },
];

function RailLink({ icon, label, count, tone }: { icon: ReactNode; label: string; count?: number; tone?: "attention" }) {
	return (
		<a href="#" className="flex items-center gap-2 rounded-md px-2 py-1.5 text-foreground/80 text-sm hover:bg-accent hover:text-foreground">
			{icon}
			<span className="flex-1">{label}</span>
			{count !== undefined && (
				<span
					className={cn(
						"rounded-full px-1.5 text-xs tabular-nums",
						tone === "attention" ? "bg-amber-500/15 text-amber-700 dark:text-amber-300" : "text-muted-foreground",
					)}
				>
					{count}
				</span>
			)}
		</a>
	);
}

export function RailE({ onSearch }: { onSearch: () => void }) {
	const [open, setOpen] = useState(() => new Set(PROJECTS.filter((p) => p.open).map((p) => p.name)));
	return (
		<div className="flex h-full min-h-0 flex-col">
			<div className="flex shrink-0 items-center gap-2 px-3 pt-3 pb-2">
				<span className="grid size-6 place-items-center rounded-md bg-foreground font-bold text-[0.6875rem] text-background">K</span>
				<span className="flex-1 truncate font-semibold text-sm">openkestrel</span>
				<ChevronDownIcon aria-hidden className="size-3.5 text-muted-foreground" />
			</div>
			<div className="shrink-0 px-2">
				<button
					type="button"
					onClick={onSearch}
					className="flex w-full items-center gap-2 rounded-md border bg-muted/40 px-2 py-1.5 text-muted-foreground text-sm hover:text-foreground"
				>
					<SearchIcon aria-hidden className="size-3.5" />
					<span className="flex-1 text-left">Search</span>
					<kbd className="font-sans text-xs">⌘K</kbd>
				</button>
			</div>
			<nav aria-label="Organization" className="grid shrink-0 gap-px px-2 pt-2">
				<RailLink icon={<InboxIcon aria-hidden className="size-4" />} label="Waiting for you" count={2} tone="attention" />
				<RailLink icon={<LayersIcon aria-hidden className="size-4" />} label="Fleet" count={4} />
				<RailLink icon={<WorkflowIcon aria-hidden className="size-4" />} label="Campaigns" count={1} />
				<RailLink icon={<ZapIcon aria-hidden className="size-4" />} label="Triggers" />
			</nav>
			<nav aria-label="Projects" className="mt-3 min-h-0 flex-1 overflow-y-auto px-2">
				<p className="flex items-center justify-between px-2 pb-1 text-muted-foreground text-xs">
					Projects
					<PlusIcon aria-label="Declare a Project" className="size-3.5" />
				</p>
				{PROJECTS.map((project) => {
					const expanded = open.has(project.name);
					return (
						<div key={project.name} className="mb-1">
							<button
								type="button"
								aria-expanded={expanded}
								onClick={() =>
									setOpen((all) => {
										const next = new Set(all);
										if (expanded) next.delete(project.name);
										else next.add(project.name);
										return next;
									})
								}
								className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-sm hover:bg-accent"
							>
								<ChevronRightIcon aria-hidden className={cn("size-3.5 text-muted-foreground transition-transform", expanded && "rotate-90")} />
								<FolderIcon aria-hidden className="size-3.5 text-muted-foreground" />
								<span className="flex-1 font-medium">{project.name}</span>
								{!expanded && <span className="text-muted-foreground text-xs">{project.workspaces.length}</span>}
							</button>
							{expanded &&
								project.workspaces.map((workspace) => (
									<a
										key={workspace.name}
										href="#"
										aria-current={workspace.current ? "page" : undefined}
										className="flex items-center gap-2 rounded-md py-1.5 pr-2 pl-7 text-sm hover:bg-accent aria-[current=page]:bg-accent"
									>
										<PhaseDot phase={workspace.phase} />
										<span
											className={cn(
												"min-w-0 flex-1 truncate",
												workspace.phase === "waiting" ? "font-semibold" : "text-foreground/80",
											)}
										>
											{workspace.name}
										</span>
										<SourceMark source={workspace.source} className="text-muted-foreground" />
										<span className="w-6 shrink-0 text-right text-muted-foreground text-xs">{workspace.ago}</span>
									</a>
								))}
						</div>
					);
				})}
			</nav>
			<div className="flex shrink-0 items-center gap-2 border-t px-3 py-2 text-xs">
				<span className="grid size-6 place-items-center rounded-full bg-muted font-medium text-[0.625rem]">JA</span>
				<span className="flex-1">jack</span>
				<span className="text-muted-foreground tabular-nums">$12.40 today</span>
			</div>
		</div>
	);
}

function WorkItemChip({ source, label, state, tone }: { source: Source; label: string; state: string; tone?: "ok" | "busy" | "bad" }) {
	return (
		<a
			href="#"
			className="inline-flex items-center gap-1.5 rounded-md border px-1.5 py-0.5 text-xs hover:bg-accent"
		>
			<SourceMark source={source} className="text-muted-foreground" />
			<span className="font-medium">{label}</span>
			<span
				className={cn(
					"text-muted-foreground",
					tone === "ok" && "text-emerald-600 dark:text-emerald-400",
					tone === "busy" && "text-amber-600 dark:text-amber-400",
					tone === "bad" && "text-red-600 dark:text-red-400",
				)}
			>
				{state}
			</span>
		</a>
	);
}

function WorkspaceHeader() {
	return (
		<header className="grid shrink-0 gap-1.5 border-b px-4 py-2.5">
			<p className="flex items-center gap-1 text-muted-foreground text-xs">
				<span>kestrel</span>
				<ChevronRightIcon aria-hidden className="size-3" />
				<WorkflowIcon aria-hidden className="size-3" />
				<span>Stabilize CI</span>
				<ChevronRightIcon aria-hidden className="size-3" />
				<span className="text-foreground">flaky-usage-test</span>
			</p>
			<div className="flex items-center gap-2">
				<h1 className="min-w-0 truncate font-semibold text-sm">Fix the flaky usage summary test</h1>
				<span className="inline-flex shrink-0 items-center gap-1.5 text-amber-700 text-xs dark:text-amber-300">
					<PhaseDot phase="waiting" />
					Waiting on an Approval
				</span>
				<span className="ml-auto flex shrink-0 -space-x-1.5" aria-label="Watching: sam and 1 anonymous">
					{["SA", "+1"].map((who) => (
						<span key={who} className="grid size-6 place-items-center rounded-full border-2 border-background bg-muted font-medium text-[0.625rem]">
							{who}
						</span>
					))}
				</span>
			</div>
			<div className="flex flex-wrap items-center gap-1.5">
				<WorkItemChip source="linear" label="KES-142" state="In Progress" tone="busy" />
				<WorkItemChip source="github" label="#812" state="checks running" tone="busy" />
				<WorkItemChip source="slack" label="#eng-ci" state="thread" />
				<span className="inline-flex items-center gap-1 font-mono text-[0.6875rem] text-muted-foreground">
					<GitBranchIcon aria-hidden className="size-3" />
					jack/flaky-usage
				</span>
			</div>
		</header>
	);
}

function TriggeredBrief() {
	const brief = FLOW.find((item) => item.kind === "brief");
	if (brief?.kind !== "brief") return null;
	return (
		<div className="grid gap-2">
			<p className="flex flex-wrap items-center gap-1.5 text-muted-foreground text-xs">
				<ZapIcon aria-hidden className="size-3" />
				Trigger <span className="font-medium text-foreground">flaky-tests</span> fired on
				<SourceMark source="linear" />
				<span className="font-medium text-foreground">KES-142</span> labelled
				<code className="rounded bg-muted px-1 font-mono">agent</code>
			</p>
			<div className="rounded-lg border bg-muted/30 px-3 py-2 text-sm">
				<p className="mb-1 text-muted-foreground text-xs uppercase tracking-wide">Brief</p>
				{brief.text.replace(/`/g, "")}
			</div>
		</div>
	);
}

function Enqueued() {
	return (
		<div className="flex items-center gap-2 rounded-lg border border-dashed px-3 py-2 text-sm">
			<WorkflowIcon aria-hidden className="size-4 shrink-0 text-muted-foreground" />
			<span className="min-w-0 flex-1">
				<span className="text-muted-foreground">opencode enqueued </span>
				<a href="#" className="font-medium underline-offset-2 hover:underline">
					cli-follower-fix
				</a>
				<span className="text-muted-foreground"> in Stabilize CI: the CLI follower drops late usage the same way</span>
			</span>
			<span className="inline-flex shrink-0 items-center gap-1.5 text-muted-foreground text-xs">
				<PhaseDot phase="queued" />
				queued #1
			</span>
		</div>
	);
}

type Unit = {
	id: string;
	title: string;
	kind: "subagent" | "background_task";
	status: "done" | "running";
	steps?: string[];
	result?: string;
	seconds?: number;
	startedAt?: string;
	progress?: number;
	of?: number;
};

const UNITS: Unit[] = [
	{
		id: "u1",
		title: "Find every follower that reads session_state",
		kind: "subagent",
		status: "done",
		steps: ["Search for apply(", "Read crates/kestrel-cli/src/follow.rs", "Read packages/client/src/operator/follow.ts"],
		result: "3 followers: the control plane's, the CLI's, the browser's. The CLI's has the same early return.",
		seconds: 38,
	},
	{
		id: "u2",
		title: "Check whether rounding finished_at can yield None",
		kind: "subagent",
		status: "running",
		steps: ["Read crates/kestrel/src/session/usage.rs", "Search for round_to_second"],
		startedAt: "2026-10-04T14:05:58Z",
	},
	{
		id: "u3",
		title: "cargo test --test usage usage_summary × 200",
		kind: "background_task",
		status: "running",
		progress: 137,
		of: 200,
		startedAt: "2026-10-04T14:05:41Z",
	},
];

function UnitLane({ unit }: { unit: Unit }) {
	const [open, setOpen] = useState(unit.status === "running" && unit.kind === "subagent");
	const since = useTicking(Date.parse(unit.startedAt ?? "2026-10-04T14:06:00Z"));
	const running = unit.status === "running";
	return (
		<div className="relative pl-5">
			<span aria-hidden className="absolute top-0 bottom-0 left-[0.4375rem] w-px bg-border" />
			<span aria-hidden className="absolute top-3.5 left-[0.4375rem] h-px w-3 bg-border" />
			<button
				type="button"
				onClick={() => setOpen(!open)}
				aria-expanded={open}
				className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left text-[0.8125rem] hover:bg-accent"
			>
				{unit.kind === "subagent" ? (
					<BotIcon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
				) : (
					<TerminalIcon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
				)}
				<span className="min-w-0 flex-1 truncate">
					{running ? <Shimmer duration={2}>{unit.title}</Shimmer> : unit.title}
				</span>
				{unit.progress !== undefined && unit.of !== undefined && (
					<span className="flex shrink-0 items-center gap-1.5 text-muted-foreground text-xs tabular-nums">
						<span className="h-1 w-16 overflow-hidden rounded-full bg-muted">
							<span className="block h-full bg-emerald-500" style={{ width: `${(unit.progress / unit.of) * 100}%` }} />
						</span>
						{unit.progress}/{unit.of} · 0 failed
					</span>
				)}
				<span className="shrink-0 text-muted-foreground text-xs tabular-nums">
					{running ? since : `${unit.seconds ?? 0}s`}
				</span>
				{running ? (
					<LoaderIcon aria-label="running" className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
				) : (
					<CheckIcon aria-label="done" className="size-3.5 shrink-0 text-emerald-600" />
				)}
			</button>
			{open && unit.steps && (
				<div className="mb-1 ml-6 grid gap-0.5 border-l pl-3 text-muted-foreground text-xs">
					{unit.steps.map((step) => (
						<span key={step} className="truncate">
							{step}
						</span>
					))}
					{unit.result && <span className="mt-1 text-foreground">{unit.result}</span>}
					{running && <Shimmer duration={1.5}>thinking…</Shimmer>}
				</div>
			)}
		</div>
	);
}

function Units() {
	return (
		<div className="grid gap-0.5">
			<div className="flex items-center gap-1.5 text-muted-foreground text-sm">
				<BotIcon aria-hidden className="size-3.5" />
				<Shimmer duration={1.5}>Working</Shimmer>
				<span>· 2 sub-agents, 1 background task</span>
			</div>
			{UNITS.map((unit) => (
				<UnitLane key={unit.id} unit={unit} />
			))}
		</div>
	);
}

function ApprovalView() {
	const [approval, setApproval] = useState<{ id: string; approved?: boolean }>({ id: "ap_1" });
	const state = approval.approved === undefined ? "approval-requested" : "approval-responded";
	return (
		<Confirmation
			approval={approval.approved === undefined ? { id: approval.id } : { id: approval.id, approved: approval.approved }}
			state={state}
			className="rounded-lg border-amber-500/40 bg-amber-500/5"
		>
			<div className="flex items-center gap-2 text-xs">
				<ShieldCheckIcon aria-hidden className="size-3.5 text-amber-600" />
				<span className="font-medium">Approval</span>
				<span className="text-muted-foreground">
					Policy <code className="font-mono">protected-push</code> · jack or sam may answer · also in Slack #eng-ci
				</span>
			</div>
			<ConfirmationTitle>
				<ConfirmationRequest>
					opencode wants to run <code className="rounded bg-muted px-1 font-mono text-xs">git push --force-with-lease origin jack/flaky-usage</code>
				</ConfirmationRequest>
				<ConfirmationAccepted>
					<span className="text-emerald-700 dark:text-emerald-400">You allowed the push.</span>
				</ConfirmationAccepted>
				<ConfirmationRejected>
					<span className="text-red-700 dark:text-red-400">You rejected the push.</span>
				</ConfirmationRejected>
			</ConfirmationTitle>
			<ConfirmationActions>
				<ConfirmationAction variant="outline" onClick={() => setApproval({ id: "ap_1", approved: false })}>
					Reject
				</ConfirmationAction>
				<ConfirmationAction onClick={() => setApproval({ id: "ap_1", approved: true })}>Allow once</ConfirmationAction>
			</ConfirmationActions>
		</Confirmation>
	);
}

const CHOICES = [
	{ value: "here", label: "Fix it here", hint: "One more commit on jack/flaky-usage" },
	{ value: "linear", label: "Open a Linear issue", hint: "Through the Linear Integration, labelled ci" },
	{ value: "leave", label: "Leave it", hint: "" },
];

// A one-part Question answers inline; Questionnaire is kept for a Question with several parts.
function QuestionView() {
	const [answer, setAnswer] = useState<string | null>(null);
	const [own, setOwn] = useState("");
	const chosen = CHOICES.find((choice) => choice.value === answer);
	return (
		<div className="grid gap-2 rounded-lg border border-sky-500/30 bg-sky-500/5 px-3 py-2.5">
			<p className="flex items-center gap-2 text-xs">
				<SparklesIcon aria-hidden className="size-3.5 text-sky-600" />
				<span className="font-medium">Question</span>
				<span className="text-muted-foreground">opencode proceeds on its own judgment in 14m</span>
			</p>
			<p className="text-sm">
				Rounding <code className="font-mono text-[0.875em]">finished_at</code> can't produce None, but it skews
				cost. What should I do about it?
			</p>
			{chosen ? (
				<p className="flex items-center gap-1.5 text-sm">
					<CheckIcon aria-hidden className="size-3.5 text-emerald-600" />
					You answered: {chosen.label}
					<button type="button" onClick={() => setAnswer(null)} className="text-muted-foreground text-xs underline-offset-2 hover:underline">
						change
					</button>
				</p>
			) : (
				<div className="flex flex-wrap items-center gap-1.5">
					{CHOICES.map((choice, index) => (
						<button
							key={choice.value}
							type="button"
							title={choice.hint || undefined}
							onClick={() => setAnswer(choice.value)}
							className="inline-flex items-center gap-1.5 rounded-md border bg-background px-2 py-1 text-xs hover:bg-accent"
						>
							<kbd className="font-sans text-[0.625rem] text-muted-foreground">{index + 1}</kbd>
							{choice.label}
						</button>
					))}
					<input
						value={own}
						onChange={(event) => setOwn(event.target.value)}
						placeholder="or answer in your own words"
						aria-label="Answer in your own words"
						className="min-w-40 flex-1 rounded-md border bg-background px-2 py-1 text-xs outline-none focus:border-ring"
					/>
				</div>
			)}
		</div>
	);
}

function TranscriptE() {
	const activities = FLOW.filter((item): item is ActivityGroup => item.kind === "activity");
	const said = FLOW.filter((item): item is Said => item.kind === "said");
	const [first] = activities;
	const [agent, sam, me] = said;
	return (
		<Conversation>
			<ConversationContent className="mx-auto w-full max-w-3xl gap-5 px-5 py-6">
				<TriggeredBrief />
				{first && <ActivityView activity={first} />}
				{agent && <AgentMessage said={agent} changed={[{ path: "crates/kestrel/tests/usage.rs", added: 12, removed: 0 }]} />}
				<Enqueued />
				{sam && <PersonMessage said={sam} />}
				{me && <PersonMessage said={me} />}
				<Units />
				<ApprovalView />
				<QuestionView />
			</ConversationContent>
			<ConversationScrollButton />
		</Conversation>
	);
}

function ComposerE() {
	return (
		<PromptInputProvider>
			<div className="shrink-0 px-3 pb-3">
				<PromptInput onSubmit={(_, event) => event.preventDefault()} className="rounded-xl">
					<HeldLine />
					<Commands />
					<PromptInputBody>
						<PromptInputTextarea placeholder="Message the Session — @ to mention, / for commands" className="min-h-14 text-sm" />
					</PromptInputBody>
					<PromptInputFooter>
						<PromptInputTools>
							<span className="inline-flex items-center gap-1 px-1.5 text-muted-foreground text-xs">
								<BotIcon aria-hidden className="size-3.5" />
								opencode
							</span>
							<ModelPicker compact />
							<span className="text-muted-foreground text-xs">high</span>
							<PromptInputButton variant="ghost" size="sm" className="w-auto gap-1 px-2 text-xs">
								<SparklesIcon className="size-3.5" />2 skills
							</PromptInputButton>
							<UsageContext />
						</PromptInputTools>
						<span className="flex shrink-0 items-center gap-1">
							<PromptInputButton variant="ghost" size="sm" className="w-auto px-2 text-xs">
								<SquareIcon className="size-3" />
								Interrupt
							</PromptInputButton>
							<PromptInputSubmit className="rounded-lg">
								<ArrowUpIcon className="size-4" />
							</PromptInputSubmit>
						</span>
					</PromptInputFooter>
				</PromptInput>
			</div>
		</PromptInputProvider>
	);
}

const CAMPAIGN_NODES = [
	{ id: "triage", name: "triage-flaky-tests", phase: "ended" as Phase, note: "found 2 flakes", x: 0, y: 1 },
	{ id: "usage", name: "flaky-usage-test", phase: "waiting" as Phase, note: "waiting on an Approval", x: 1, y: 0 },
	{ id: "cli", name: "cli-follower-fix", phase: "queued" as Phase, note: "queued #1", x: 1, y: 2 },
	{ id: "notes", name: "release-notes", phase: "blocked" as Phase, note: "after both", x: 2, y: 1 },
];
const CAMPAIGN_EDGES = [
	["triage", "usage"],
	["triage", "cli"],
	["usage", "notes"],
	["cli", "notes"],
] as const;

function CampaignView() {
	const W = 168;
	const H = 60;
	const GX = 40;
	const GY = 28;
	const pos = (id: string) => {
		const node = CAMPAIGN_NODES.find((one) => one.id === id);
		return { x: (node?.x ?? 0) * (W + GX), y: (node?.y ?? 0) * (H + GY) };
	};
	return (
		<div className="grid min-h-0 gap-4 overflow-auto p-5">
			<div className="flex flex-wrap items-center gap-3">
				<WorkflowIcon aria-hidden className="size-4 text-muted-foreground" />
				<h2 className="font-semibold text-sm">Stabilize CI</h2>
				<span className="text-muted-foreground text-xs">
					Workflow <code className="font-mono">ci-stabilizer</code> · started by Trigger flaky-tests
				</span>
				<span className="ml-auto flex gap-1.5">
					<button type="button" className="inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-accent">
						<PauseIcon className="size-3" />
						Pause
					</button>
					<button type="button" className="inline-flex items-center gap-1 rounded-md border px-2 py-1 text-red-600 text-xs hover:bg-accent">
						<XIcon className="size-3" />
						Cancel
					</button>
				</span>
			</div>
			<dl className="flex flex-wrap gap-x-6 gap-y-1 text-xs">
				<div>
					<dt className="text-muted-foreground">Concurrency</dt>
					<dd className="tabular-nums">1 of 2 running</dd>
				</div>
				<div>
					<dt className="text-muted-foreground">Spend</dt>
					<dd className="flex items-center gap-2 tabular-nums">
						$14.20 of $40
						<span className="h-1 w-20 overflow-hidden rounded-full bg-muted">
							<span className="block h-full w-[35%] bg-foreground/70" />
						</span>
					</dd>
				</div>
				<div>
					<dt className="text-muted-foreground">Roster</dt>
					<dd>opencode, claude-reviewer</dd>
				</div>
			</dl>
			<div className="relative" style={{ width: 3 * W + 2 * GX, height: 3 * H + 2 * GY }}>
				<svg aria-hidden className="absolute inset-0 size-full overflow-visible text-border">
					{CAMPAIGN_EDGES.map(([from, to]) => {
						const a = pos(from);
						const b = pos(to);
						const x1 = a.x + W;
						const y1 = a.y + H / 2;
						const x2 = b.x;
						const y2 = b.y + H / 2;
						return (
							<path
								key={`${from}-${to}`}
								d={`M${x1},${y1} C${x1 + GX / 2},${y1} ${x2 - GX / 2},${y2} ${x2},${y2}`}
								fill="none"
								stroke="currentColor"
								strokeWidth={1.5}
							/>
						);
					})}
				</svg>
				{CAMPAIGN_NODES.map((node) => {
					const p = pos(node.id);
					return (
						<a
							key={node.id}
							href="#"
							className={cn(
								"absolute grid content-center gap-0.5 rounded-lg border bg-background px-3 shadow-xs hover:bg-accent",
								node.id === "usage" && "ring-2 ring-amber-500/50",
							)}
							style={{ left: p.x, top: p.y, width: W, height: H }}
						>
							<span className="flex items-center gap-2 font-medium text-sm">
								<PhaseDot phase={node.phase} />
								<span className="truncate">{node.name}</span>
							</span>
							<span className="truncate text-muted-foreground text-xs">{node.note}</span>
						</a>
					);
				})}
			</div>
		</div>
	);
}

const MAIN_TABS = [
	{ id: "usage", label: "flaky-usage-test", phase: "waiting" as Phase, icon: null },
	{ id: "pr", label: "pr-818-review", phase: "working" as Phase, icon: null },
	{ id: "campaign", label: "Stabilize CI", phase: null, icon: <WorkflowIcon aria-hidden className="size-3.5" /> },
];

function SidebarToggle({ side }: { side: keyof Collapsed }) {
	const { collapsed, toggle } = useContext(Sidebars);
	const Icon = side === "workspaces" ? PanelLeftIcon : PanelRightIcon;
	return (
		<button
			type="button"
			aria-pressed={!collapsed[side]}
			aria-label={side === "workspaces" ? "Toggle the sidebar (⌘B)" : "Toggle the inspector (⌘⌥B)"}
			onClick={() => toggle(side)}
			className="hidden shrink-0 rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground workbench:block"
		>
			<Icon className="size-4" />
		</button>
	);
}

export function MainE() {
	const [tab, setTab] = useState("usage");
	return (
		<Tabs value={tab} onValueChange={setTab} className="flex h-full min-h-0 flex-col gap-0">
			<div className="flex shrink-0 items-center border-b px-1">
				<SidebarToggle side="workspaces" />
				<TabsList variant="line" className="h-10 min-w-0 justify-start gap-0 overflow-x-auto rounded-none bg-transparent p-0" aria-label="Open tabs">
					{MAIN_TABS.map((one) => (
						<TabsTrigger key={one.id} value={one.id} className="h-10 max-w-48 flex-none gap-1.5 rounded-none px-3 text-xs">
							{one.phase ? <PhaseDot phase={one.phase} /> : one.icon}
							<span className="truncate">{one.label}</span>
							<XIcon aria-hidden className="size-3 text-muted-foreground opacity-60" />
						</TabsTrigger>
					))}
				</TabsList>
				<button type="button" aria-label="Open a Workspace" className="ml-1 rounded-md p-1.5 text-muted-foreground hover:bg-accent">
					<PlusIcon className="size-3.5" />
				</button>
				<span className="flex-1" />
				<SidebarToggle side="work" />
			</div>
			<TabsContent value="usage" className="flex min-h-0 flex-1 flex-col">
				<WorkspaceHeader />
				<TranscriptE />
				<ComposerE />
			</TabsContent>
			<TabsContent value="pr" className="grid flex-1 place-items-center text-muted-foreground text-sm">
				Another Workspace, kept open beside this one.
			</TabsContent>
			<TabsContent value="campaign" className="min-h-0 flex-1 overflow-auto">
				<CampaignView />
			</TabsContent>
		</Tabs>
	);
}

const CHECKS = [
	{ name: "fmt", state: "ok", time: "12s" },
	{ name: "clippy", state: "ok", time: "2m 41s" },
	{ name: "test (ubuntu)", state: "busy", time: "4m 02s" },
	{ name: "test (macos)", state: "bad", time: "6m 10s", note: "usage_summary_after_trailing_turn" },
	{ name: "client typecheck", state: "ok", time: "48s" },
];

function ChecksView() {
	return (
		<div className="grid gap-3 py-2">
			<p className="flex items-center gap-2 px-4 text-xs">
				<GitPullRequestIcon aria-hidden className="size-3.5 text-muted-foreground" />
				<a href="#" className="font-medium hover:underline">
					#812 Keep the usage a trailing agent reports
				</a>
			</p>
			<ul>
				{CHECKS.map((check) => (
					<li key={check.name} className="flex items-start gap-2 px-4 py-1.5 text-sm">
						{check.state === "ok" ? (
							<CheckIcon aria-label="passed" className="mt-0.5 size-3.5 shrink-0 text-emerald-600" />
						) : check.state === "busy" ? (
							<LoaderIcon aria-label="running" className="mt-0.5 size-3.5 shrink-0 animate-spin text-amber-600" />
						) : (
							<XIcon aria-label="failed" className="mt-0.5 size-3.5 shrink-0 text-red-600" />
						)}
						<span className="min-w-0 flex-1">
							{check.name}
							{check.note && <span className="block truncate font-mono text-muted-foreground text-xs">{check.note}</span>}
						</span>
						<span className="shrink-0 text-muted-foreground text-xs tabular-nums">{check.time}</span>
					</li>
				))}
			</ul>
			<p className="px-4 text-muted-foreground text-xs">From the GitHub Integration · refreshed 20s ago</p>
		</div>
	);
}

function TasksView() {
	const plan = [
		{ text: "Reproduce the flake under load", done: true },
		{ text: "Find where the follower drops late usage", done: true },
		{ text: "Keep usage that trails session_ended", done: true },
		{ text: "Run the test 200 times", done: false, active: true },
		{ text: "Push and open the pull request", done: false },
	];
	const acceptance = [
		{ text: "usage_summary passes 200/200 on CI", done: false },
		{ text: "No sleep added to the test", done: true },
	];
	const List = ({ items }: { items: { text: string; done: boolean; active?: boolean }[] }) => (
		<ul className="grid gap-0.5">
			{items.map((item) => (
				<li key={item.text} className="flex items-start gap-2 px-4 py-1 text-sm">
					{item.done ? (
						<CheckIcon aria-label="done" className="mt-0.5 size-3.5 shrink-0 text-emerald-600" />
					) : item.active ? (
						<LoaderIcon aria-label="in progress" className="mt-0.5 size-3.5 shrink-0 animate-spin text-muted-foreground" />
					) : (
						<CircleDashedIcon aria-label="pending" className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
					)}
					<span className={cn(item.done && "text-muted-foreground line-through")}>{item.text}</span>
				</li>
			))}
		</ul>
	);
	return (
		<div className="grid gap-4 py-2">
			<section>
				<p className="px-4 pb-1 text-muted-foreground text-xs">opencode's plan</p>
				<List items={plan} />
			</section>
			<section>
				<p className="flex items-center gap-1.5 px-4 pb-1 text-muted-foreground text-xs">
					<SourceMark source="linear" />
					KES-142 acceptance
				</p>
				<List items={acceptance} />
			</section>
		</div>
	);
}

function SessionView() {
	const Row = ({ label, children }: { label: string; children: ReactNode }) => (
		<div className="flex items-baseline gap-3 px-4 py-1.5 text-sm">
			<dt className="w-24 shrink-0 text-muted-foreground text-xs">{label}</dt>
			<dd className="min-w-0 flex-1">{children}</dd>
		</div>
	);
	return (
		<dl className="grid py-2">
			<Row label="Agent">opencode · Claude Opus 5.5 · high</Row>
			<Row label="Harness">opencode 1.4.2</Row>
			<Row label="Environment">
				<span className="font-mono text-xs">rust-1.92</span> <span className="text-muted-foreground text-xs">from devcontainer.json</span>
			</Row>
			<Row label="Instance">
				<span className="inline-flex items-center gap-1.5">
					<ServerIcon aria-hidden className="size-3.5 text-muted-foreground" />
					docker · up 42m · 4 CPU
				</span>
			</Row>
			<Row label="Skills">
				<span className="grid gap-0.5 text-xs">
					<span>
						rust-tests <span className="text-muted-foreground">v3 · organization</span>
					</span>
					<span>
						AGENTS.md <span className="text-muted-foreground">repository</span>
					</span>
				</span>
			</Row>
			<Row label="MCP">
				<span className="grid gap-0.5 text-xs">
					<span>
						linear <span className="text-muted-foreground">read · per-Session credential</span>
					</span>
					<span>
						kestrel <span className="text-muted-foreground">pending Events</span>
					</span>
				</span>
			</Row>
			<Row label="Policy">
				<span className="text-xs">
					ci-agents <span className="text-muted-foreground">∩ organization ceiling · 3 decisions audited</span>
				</span>
			</Row>
		</dl>
	);
}

function ShellView() {
	return (
		<div className="grid gap-2 p-3">
			<p className="text-muted-foreground text-xs">Instance shell · every command is audited under Policy</p>
			<Terminal
				className="text-xs"
				output={"$ cargo nextest run -p kestrel usage\n    Starting 4 tests across 1 binary\n        PASS [   3.102s] kestrel::usage usage_summary_after_trailing_turn\n$ "}
			/>
		</div>
	);
}

const INSPECTOR = [
	{ id: "changes", label: "Changes" },
	{ id: "checks", label: "Checks" },
	{ id: "tasks", label: "Tasks" },
	{ id: "session", label: "Session" },
	{ id: "shell", label: "Shell" },
] as const;

export function InspectorE() {
	const [tab, setTab] = useState<string>("checks");
	return (
		<Tabs value={tab} onValueChange={setTab} className="flex h-full min-h-0 flex-col gap-0">
			<TabsList variant="line" className="h-10 w-full shrink-0 justify-start gap-0 overflow-x-auto rounded-none border-b bg-transparent px-1" aria-label="Inspector">
				{INSPECTOR.map((one) => (
					<TabsTrigger key={one.id} value={one.id} className="h-10 flex-none px-2.5 text-xs">
						{one.label}
						{one.id === "checks" && <span className="size-1.5 rounded-full bg-red-500" aria-label="a check failed" />}
					</TabsTrigger>
				))}
			</TabsList>
			<TabsContent value="changes" className="min-h-0 flex-1 overflow-y-auto">
				<ChangesView />
			</TabsContent>
			<TabsContent value="checks" className="min-h-0 flex-1 overflow-y-auto">
				<ChecksView />
			</TabsContent>
			<TabsContent value="tasks" className="min-h-0 flex-1 overflow-y-auto">
				<TasksView />
			</TabsContent>
			<TabsContent value="session" className="min-h-0 flex-1 overflow-y-auto">
				<SessionView />
			</TabsContent>
			<TabsContent value="shell" className="min-h-0 flex-1 overflow-y-auto">
				<ShellView />
			</TabsContent>
		</Tabs>
	);
}

export function PaletteE({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "k" && (event.metaKey || event.ctrlKey)) {
				event.preventDefault();
				onOpenChange(!open);
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [open, onOpenChange]);
	const item = "gap-2";
	return (
		<CommandDialog open={open} onOpenChange={onOpenChange} title="Search kestrel" className="rounded-xl sm:max-w-xl">
			<Command>
			<CommandInput placeholder="Search Workspaces, Transcripts, Work Items, or run an action…" />
			<CommandList className="max-h-96">
				<CommandEmpty>Nothing matches.</CommandEmpty>
				<CommandGroup heading="Waiting for you">
					<CommandItem className={item}>
						<ShieldCheckIcon className="size-4 text-amber-600" />
						Approve a force push in flaky-usage-test
						<CommandShortcut>Approval</CommandShortcut>
					</CommandItem>
					<CommandItem className={item}>
						<SparklesIcon className="size-4 text-sky-600" />
						Answer: what to do about rounding finished_at
						<CommandShortcut>Question</CommandShortcut>
					</CommandItem>
				</CommandGroup>
				<CommandGroup heading="Workspaces">
					{PROJECTS.flatMap((project) =>
						project.workspaces.map((workspace) => (
							<CommandItem key={workspace.name} className={item} value={`${project.name} ${workspace.name}`}>
								<PhaseDot phase={workspace.phase} />
								{workspace.name}
								<span className="text-muted-foreground text-xs">{project.name}</span>
								<CommandShortcut>{workspace.ago}</CommandShortcut>
							</CommandItem>
						)),
					)}
				</CommandGroup>
				<CommandGroup heading="In Transcripts">
					<CommandItem className={item} value="transcript usage follower session_ended">
						<ListChecksIcon className="size-4" />
						<span className="truncate">“…the follower stops applying session_state once it has seen session_ended…”</span>
						<CommandShortcut>flaky-usage-test</CommandShortcut>
					</CommandItem>
				</CommandGroup>
				<CommandGroup heading="Work Items">
					<CommandItem className={item} value="KES-142 usage summary flakes linear">
						<SourceMark source="linear" />
						KES-142 Usage summary flakes on CI
						<CommandShortcut>In Progress</CommandShortcut>
					</CommandItem>
					<CommandItem className={item} value="812 pull request keep usage github">
						<SourceMark source="github" />
						#812 Keep the usage a trailing agent reports
						<CommandShortcut>open</CommandShortcut>
					</CommandItem>
				</CommandGroup>
				<CommandGroup heading="Actions">
					<CommandItem className={item}>
						<PlusIcon className="size-4" />
						Open a Workspace in kestrel…
						<CommandShortcut>⌘N</CommandShortcut>
					</CommandItem>
					<CommandItem className={item}>
						<ZapIcon className="size-4" />
						Dry-run Trigger flaky-tests against an Event…
					</CommandItem>
					<CommandItem className={item}>
						<TerminalIcon className="size-4" />
						Open the Instance shell
					</CommandItem>
				</CommandGroup>
			</CommandList>
			</Command>
		</CommandDialog>
	);
}
