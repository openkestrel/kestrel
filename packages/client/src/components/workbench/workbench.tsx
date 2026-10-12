import { PanelLeft, PanelRight } from "lucide-react";
import {
	type ReactNode,
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";
import { cn } from "#/lib/utils";

const PANES = [
	{ value: "workspaces", label: "Workspaces" },
	{ value: "transcript", label: "Transcript" },
	{ value: "work", label: "Work" },
] as const;

type Pane = (typeof PANES)[number]["value"];

/** The two panes that can fold away beside the Transcript. */
type Side = Exclude<Pane, "transcript">;

type Collapsed = Partial<Record<Side, boolean>>;

type Chrome = {
	collapsed: Collapsed;
	toggle: (side: Side) => void;
	focusComposer: () => void;
	registerComposer: (input: HTMLTextAreaElement | null) => void;
};

const ChromeContext = createContext<Chrome | null>(null);

export function useWorkbenchChrome(): Chrome | null {
	return useContext(ChromeContext);
}

const SIDE_LABEL: Record<Side, string> = { workspaces: "Workspaces", work: "Work" };

// The four side-by-side shapes: a folded pane holds its toggle in a narrow rail.
const COLUMNS = {
	open: "workbench:grid-cols-workbench",
	left: "workbench:grid-cols-[2.75rem_minmax(0,1fr)_20rem]",
	right: "workbench:grid-cols-[16rem_minmax(0,1fr)_2.75rem]",
	both: "workbench:grid-cols-[2.75rem_minmax(0,1fr)_2.75rem]",
} as const;

// Offscreen until focused, when it drops into the corner a reader can see.
const SKIP =
	"absolute left-2 top-2 z-50 h-px w-px overflow-hidden whitespace-nowrap focus:h-auto focus:w-auto focus:overflow-visible focus:whitespace-normal focus:rounded-md focus:border focus:bg-background focus:px-3 focus:py-2 focus:text-sm focus:shadow-md focus:ring-2 focus:ring-ring/50";

export function ComposerSkipLink({ name = "Skip to the composer" }: { name?: string }) {
	const chrome = useWorkbenchChrome();
	return (
		<a
			data-composer-skip
			href="#composer-input"
			className={SKIP}
			onClick={(event) => {
				event.preventDefault();
				chrome?.focusComposer();
			}}
		>
			{name}
		</a>
	);
}

export function Workbench(panes: Record<Pane, ReactNode> & { initial?: Pane }) {
	const [shown, setShown] = useState<Pane>(panes.initial ?? "transcript");
	const [collapsed, setCollapsed] = useState<Collapsed>({});
	const [notice, setNotice] = useState("");
	const composer = useRef<HTMLTextAreaElement | null>(null);

	const registerComposer = useCallback((input: HTMLTextAreaElement | null) => {
		composer.current = input;
	}, []);

	const focusComposer = useCallback(() => {
		setShown("transcript");
		requestAnimationFrame(() => composer.current?.focus());
	}, []);

	const toggle = useCallback(
		(side: Side) => {
			const fold = collapsed[side] !== true;
			// A reader who was inside the pane keeps a control under their focus rather than losing it.
			if (holdsFocus(side)) {
				const where = fold ? "rail" : "heading";
				requestAnimationFrame(() => sidebarToggle(side, where)?.focus());
			}
			setCollapsed((current) => ({ ...current, [side]: fold }));
			setNotice(`${SIDE_LABEL[side]} pane ${fold ? "collapsed" : "expanded"}`);
		},
		[collapsed],
	);

	useEffect(() => {
		function onKeyDown(event: KeyboardEvent) {
			// Option+B is "∫" on macOS, so the physical key is what names the shortcut.
			if (event.repeat || (event.code !== "KeyB" && event.key.toLowerCase() !== "b")) return;
			if (!(event.metaKey || event.ctrlKey)) return;
			event.preventDefault();
			toggle(event.altKey ? "work" : "workspaces");
		}
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, [toggle]);

	const chrome = useMemo(
		() => ({ collapsed, toggle, focusComposer, registerComposer }),
		[collapsed, toggle, focusComposer, registerComposer],
	);

	return (
		<main className="flex h-dvh flex-col">
			<ChromeContext.Provider value={chrome}>
				<ComposerSkipLink />
				<Tabs
					value={shown}
					onValueChange={(value: Pane) => setShown(value)}
					className="flex min-h-0 flex-1 flex-col gap-0"
				>
					<TabsList className="w-full shrink-0 workbench:hidden" aria-label="Panes">
						{PANES.map(({ value, label }) => (
							<TabsTrigger key={value} value={value}>
								{label}
							</TabsTrigger>
						))}
					</TabsList>
					<div
						className={cn(
							"grid min-h-0 flex-1 grid-cols-1 workbench:divide-x",
							COLUMNS[columns(collapsed)],
						)}
					>
						{PANES.map(({ value, label }) => {
							const side = value === "transcript" ? undefined : value;
							const folded = side !== undefined && collapsed[side] === true;
							return (
								<TabsContent
									key={value}
									value={value}
									keepMounted
									// Base UI hides and inerts every inactive panel; above the breakpoint all three are shown.
									hidden={false}
									inert={false}
									className="min-h-0 overflow-y-auto data-hidden:max-workbench:hidden"
								>
									{side !== undefined && folded && (
										<div className="hidden h-full flex-col items-center pt-3 workbench:flex">
											<SidebarToggle side={side} where="rail" />
										</div>
									)}
									<section
										id={`pane-${value}`}
										data-pane={value}
										aria-label={label}
										className={cn("flex h-full min-h-0 flex-col", folded && "workbench:hidden")}
									>
										{panes[value]}
									</section>
								</TabsContent>
							);
						})}
					</div>
				</Tabs>
				<p aria-live="polite" className="sr-only" data-workbench-announcement>
					{notice}
				</p>
			</ChromeContext.Provider>
		</main>
	);
}

export function PaneHeading({
	children,
	level = 2,
	side,
}: {
	children: ReactNode;
	level?: 1 | 2;
	side?: Side;
}) {
	const Heading = level === 1 ? "h1" : "h2";
	return (
		<header className="flex min-h-12 shrink-0 items-center gap-2 border-b px-4">
			<Heading className="truncate font-semibold text-sm">{children}</Heading>
			{side !== undefined && (
				<span className="ml-auto flex">
					<SidebarToggle side={side} where="heading" />
				</span>
			)}
		</header>
	);
}

function SidebarToggle({ side, where }: { side: Side; where: "heading" | "rail" }) {
	const chrome = useWorkbenchChrome();
	if (chrome === null) return null;
	const Icon = side === "workspaces" ? PanelLeft : PanelRight;
	return (
		<button
			type="button"
			data-sidebar={where}
			data-side={side}
			aria-expanded={chrome.collapsed[side] !== true}
			aria-controls={`pane-${side}`}
			aria-keyshortcuts={side === "workspaces" ? "Meta+B" : "Meta+Alt+B"}
			aria-label={`Toggle the ${SIDE_LABEL[side]} pane`}
			onClick={() => chrome.toggle(side)}
			className="hidden size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-1 focus-visible:outline-ring workbench:inline-flex"
		>
			<Icon aria-hidden className="size-4" />
		</button>
	);
}

function columns(collapsed: Collapsed): keyof typeof COLUMNS {
	if (collapsed.workspaces && collapsed.work) return "both";
	if (collapsed.workspaces) return "left";
	if (collapsed.work) return "right";
	return "open";
}

function holdsFocus(side: Side): boolean {
	const active = document.activeElement;
	if (active === null) return false;
	return (
		active.closest(`[data-pane="${side}"]`) !== null ||
		active.closest(`[data-sidebar="rail"][data-side="${side}"]`) !== null
	);
}

function sidebarToggle(side: Side, where: "heading" | "rail"): HTMLElement | null {
	return document.querySelector<HTMLElement>(`[data-sidebar="${where}"][data-side="${side}"]`);
}
