import { type ReactNode, useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";

const PANES = [
	{ value: "workspaces", label: "Workspaces" },
	{ value: "transcript", label: "Transcript" },
	{ value: "work", label: "Work" },
] as const;

type Pane = (typeof PANES)[number]["value"];

export function Workbench(panes: Record<Pane, ReactNode> & { initial?: Pane }) {
	const [shown, setShown] = useState<Pane>(panes.initial ?? "transcript");

	return (
		<main className="flex h-dvh flex-col">
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
				<div className="grid min-h-0 flex-1 grid-cols-1 workbench:grid-cols-workbench workbench:divide-x">
					{PANES.map(({ value, label }) => (
						<TabsContent
							key={value}
							value={value}
							keepMounted
							// Base UI hides and inerts every inactive panel; above the breakpoint all three are shown.
							hidden={false}
							inert={false}
							className="min-h-0 overflow-y-auto data-hidden:max-workbench:hidden"
						>
							<section aria-label={label} className="flex h-full min-h-0 flex-col">
								{panes[value]}
							</section>
						</TabsContent>
					))}
				</div>
			</Tabs>
		</main>
	);
}

export function PaneHeading({ children, level = 2 }: { children: ReactNode; level?: 1 | 2 }) {
	const Heading = level === 1 ? "h1" : "h2";
	return (
		<header className="flex min-h-12 shrink-0 items-center gap-2 border-b px-4">
			<Heading className="truncate font-semibold text-sm">{children}</Heading>
		</header>
	);
}
