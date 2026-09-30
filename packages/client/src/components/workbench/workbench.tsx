import { type ReactNode, useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";

const PANES = [
	{ value: "workspaces", label: "Workspaces" },
	{ value: "transcript", label: "Transcript" },
	{ value: "work", label: "Work" },
] as const;

type Pane = (typeof PANES)[number]["value"];

// Below lg the panes become tabs; from lg every pane shows and the tab list is hidden.
export function Workbench(panes: Record<Pane, ReactNode> & { initial?: Pane }) {
	const [shown, setShown] = useState<Pane>(panes.initial ?? "transcript");

	return (
		<Tabs
			value={shown}
			onValueChange={(value: Pane) => setShown(value)}
			className="flex h-dvh flex-col gap-0"
		>
			<TabsList className="w-full shrink-0 lg:hidden" aria-label="Panes">
				{PANES.map(({ value, label }) => (
					<TabsTrigger key={value} value={value}>
						{label}
					</TabsTrigger>
				))}
			</TabsList>
			<div className="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-workbench lg:divide-x">
				{PANES.map(({ value, label }) => (
					<TabsContent
						key={value}
						value={value}
						keepMounted
						// Base UI hides and inerts every inactive panel; from lg all three are shown.
						hidden={false}
						inert={false}
						className="min-h-0 overflow-y-auto data-hidden:max-lg:hidden"
					>
						<section aria-label={label} className="flex h-full min-h-0 flex-col">
							{panes[value]}
						</section>
					</TabsContent>
				))}
			</div>
		</Tabs>
	);
}

export function PaneHeading({ children }: { children: ReactNode }) {
	return (
		<header className="flex min-h-12 shrink-0 items-center gap-2 border-b px-4">
			<h2 className="truncate font-semibold text-sm">{children}</h2>
		</header>
	);
}
