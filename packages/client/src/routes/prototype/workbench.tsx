// PROTOTYPE (#492): the workbench on AI Elements, fed by fixtures. /prototype/workbench?variant=A&font=inter&theme=light
import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { DiffView, FilesView, SessionsQueue, WorkspacesList } from "#/components/workbench/prototype/parts";
import { FONTS, type Font, MONOS, type Mono, PrototypeSwitcher, type Theme } from "#/components/workbench/prototype/switcher";
import { InspectorE, PaletteE, RailE } from "#/components/workbench/prototype/variant-e";
import { ChangesView, WorkspacesD } from "#/components/workbench/prototype/variant-d";
import { VARIANTS } from "#/components/workbench/prototype/variants";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";

type Variant = keyof typeof VARIANTS;

export const Route = createFileRoute("/prototype/workbench")({
	validateSearch: (search: Record<string, unknown>) => ({
		variant: (search.variant && String(search.variant) in VARIANTS ? String(search.variant) : "D") as Variant,
		font: (search.font && String(search.font) in FONTS ? String(search.font) : "geist") as Font,
		mono: (search.mono && String(search.mono) in MONOS ? String(search.mono) : "jetbrains") as Mono,
		theme: (search.theme === "dark" ? "dark" : "light") as Theme,
		pane: (search.pane === "workspaces" || search.pane === "work" ? search.pane : "transcript") as
			| "workspaces"
			| "transcript"
			| "work",
	}),
	component: PrototypeWorkbench,
});

function PrototypeWorkbench() {
	const search = Route.useSearch();
	const navigate = Route.useNavigate();
	const { Pane, name } = VARIANTS[search.variant];

	useEffect(() => {
		const root = document.documentElement;
		root.dataset.theme = search.theme;
		root.style.setProperty("--prototype-sans", FONTS[search.font].family);
		root.style.setProperty("--prototype-mono", MONOS[search.mono].family);
	}, [search.theme, search.font, search.mono]);
	const refined = search.variant === "D" || search.variant === "E";
	const aspirational = search.variant === "E";
	const [palette, setPalette] = useState(false);

	return (
		<>
			<Workbench
				key={search.variant}
				initial={search.pane}
				workspaces={
					aspirational ? (
						<RailE onSearch={() => setPalette(true)} />
					) : (
					<>
						<PaneHeading>Workspaces</PaneHeading>
						{refined ? <WorkspacesD /> : <WorkspacesList />}
					</>
					)
				}
				transcript={<Pane />}
				work={aspirational ? <InspectorE /> : <WorkPane refined={refined} />}
			/>
			{aspirational && <PaletteE open={palette} onOpenChange={setPalette} />}
			<PrototypeSwitcher
				variants={Object.keys(VARIANTS) as Variant[]}
				current={search.variant}
				name={name}
				font={search.font}
				mono={search.mono}
				theme={search.theme}
				onChange={(next) => void navigate({ search: (prev) => ({ ...prev, ...next }), replace: true })}
			/>
		</>
	);
}

const VIEWS = ["Work", "Diff", "Files", "Sessions", "People"] as const;

function WorkPane({ refined }: { refined: boolean }) {
	const [view, setView] = useState<(typeof VIEWS)[number]>("Diff");
	return (
		<>
			<PaneHeading>Work</PaneHeading>
			<Tabs value={view} onValueChange={setView} className="flex min-h-0 flex-1 flex-col gap-0">
				<TabsList className="w-full shrink-0 justify-start" aria-label="Work views">
					{VIEWS.map((value) => (
						<TabsTrigger key={value} value={value}>
							{refined && value === "Diff" ? "Changes" : value}
						</TabsTrigger>
					))}
				</TabsList>
				<TabsContent value="Diff" className="min-h-0 flex-1 overflow-y-auto">
					{refined ? <ChangesView /> : <DiffView />}
				</TabsContent>
				<TabsContent value="Files" className="min-h-0 flex-1 overflow-y-auto">
					<FilesView />
				</TabsContent>
				<TabsContent value="Sessions" className="min-h-0 flex-1 overflow-y-auto">
					<SessionsQueue />
				</TabsContent>
				{(["Work", "People"] as const).map((value) => (
					<TabsContent key={value} value={value} className="p-4 text-muted-foreground text-sm">
						Not part of this prototype.
					</TabsContent>
				))}
			</Tabs>
		</>
	);
}
