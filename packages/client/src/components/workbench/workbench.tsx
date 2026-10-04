import type React from "react";
import { type ReactNode, useState } from "react";
import { cn } from "#/lib/utils";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";

const PANES = [
	{ value: "workspaces", label: "Workspaces" },
	{ value: "transcript", label: "Transcript" },
	{ value: "work", label: "Work" },
] as const;

type Pane = (typeof PANES)[number]["value"];

export function Workbench(
	panes: Record<Pane, ReactNode> & { initial?: Pane; collapsed?: { workspaces?: boolean; work?: boolean } },
) {
	const columns = `${panes.collapsed?.workspaces ? "" : "16rem "}minmax(0, 1fr)${panes.collapsed?.work ? "" : " 20rem"}`;
	const [shown, setShown] = useState<Pane>(panes.initial ?? "transcript");

	return (
		<Tabs
			value={shown}
			onValueChange={(value: Pane) => setShown(value)}
			className="flex h-dvh flex-col gap-0"
		>
			<TabsList className="w-full shrink-0 workbench:hidden" aria-label="Panes">
				{PANES.map(({ value, label }) => (
					<TabsTrigger key={value} value={value}>
						{label}
					</TabsTrigger>
				))}
			</TabsList>
			<div
				className="grid min-h-0 flex-1 grid-cols-1 workbench:grid-cols-(--columns) workbench:divide-x"
				style={{ "--columns": columns } as React.CSSProperties}
			>
				{PANES.map(({ value, label }) => (
					<TabsContent
						key={value}
						value={value}
						keepMounted
						// Base UI hides and inerts every inactive panel; above the breakpoint all three are shown.
						hidden={false}
						inert={false}
						className={cn(
							"min-h-0 overflow-y-auto data-hidden:max-workbench:hidden",
							value !== "transcript" && panes.collapsed?.[value] && "workbench:hidden",
						)}
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
