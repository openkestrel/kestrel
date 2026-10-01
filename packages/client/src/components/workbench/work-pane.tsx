import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";
import type { Currency } from "#/operator/currency";
import type { Workspace } from "#/operator/generated";
import { DiffTab } from "./work/diff-tab";
import { FilesTab } from "./work/files-tab";
import { PeopleTab } from "./work/people-tab";
import { SessionsTab } from "./work/sessions-tab";
import { WorkTab } from "./work/work-tab";
import { PaneHeading } from "./workbench";

const VIEWS = [
	{ value: "work", label: "Work" },
	{ value: "diff", label: "Diff" },
	{ value: "files", label: "Files" },
	{ value: "sessions", label: "Sessions" },
	{ value: "people", label: "People" },
] as const;

type View = (typeof VIEWS)[number]["value"];

export function WorkPane({
	currency,
	organization,
	workspace,
	record,
}: {
	currency: Currency;
	organization: string;
	workspace: string;
	record: Workspace | undefined;
}) {
	const [view, setView] = useState<View>("work");

	return (
		<>
			<PaneHeading>Work</PaneHeading>
			<Tabs
				value={view}
				onValueChange={(value: View) => setView(value)}
				className="flex min-h-0 flex-1 flex-col gap-0"
			>
				<TabsList className="w-full shrink-0 justify-start" aria-label="Work views">
					{VIEWS.map(({ value, label }) => (
						<TabsTrigger key={value} value={value}>
							{label}
						</TabsTrigger>
					))}
				</TabsList>
				<TabsContent value="work" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<WorkTab organization={organization} workspace={workspace} record={record} />
				</TabsContent>
				<TabsContent value="diff" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<DiffTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="files" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<FilesTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="sessions" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<SessionsTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="people" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<PeopleTab currency={currency} organization={organization} workspace={workspace} />
				</TabsContent>
			</Tabs>
		</>
	);
}
