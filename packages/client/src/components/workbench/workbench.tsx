import { type ReactNode, useId, useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";

const PANES = [
	{ value: "workspaces", label: "Workspaces" },
	{ value: "transcript", label: "Transcript" },
	{ value: "work", label: "Work" },
] as const;

type Pane = (typeof PANES)[number]["value"];

// Under 900px the panes become tabs; above it every pane shows and the tab list is hidden.
export function Workbench(panes: Record<Pane, ReactNode> & { initial?: Pane }) {
	const [shown, setShown] = useState<Pane>(panes.initial ?? "transcript");
	const id = useId();

	return (
		<Tabs
			value={shown}
			onValueChange={(value) => setShown(value as Pane)}
			className="flex h-dvh flex-col gap-0"
		>
			<TabsList className="w-full shrink-0 rounded-none min-[900px]:hidden" aria-label="Panes">
				{PANES.map(({ value, label }) => (
					<TabsTrigger key={value} value={value} id={`${id}-${value}-tab`}>
						{label}
					</TabsTrigger>
				))}
			</TabsList>
			<div className="grid min-h-0 flex-1 grid-cols-1 min-[900px]:grid-cols-[16rem_minmax(0,1fr)_20rem] min-[900px]:divide-x">
				{PANES.map(({ value, label }) => (
					<TabsContent
						key={value}
						value={value}
						forceMount
						aria-labelledby={`${id}-${value}-tab`}
						className="min-h-0 overflow-y-auto data-[state=inactive]:max-[899px]:hidden"
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
