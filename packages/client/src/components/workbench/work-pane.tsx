import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#/components/ui/tabs";
import type { Workspace } from "#/operator/generated";
import type { TranscriptSnapshot } from "#/operator/transcript";
import { ChangesTab } from "./work/changes-tab";
import { FilesTab } from "./work/files-tab";
import { PeopleTab } from "./work/people-tab";
import { SessionsTab } from "./work/sessions-tab";
import { WorkTab } from "./work/work-tab";
import { ComposerSkipLink, PaneHeading } from "./workbench";

const VIEWS = [
	{ value: "work", label: "Work" },
	{ value: "changes", label: "Changes" },
	{ value: "files", label: "Files" },
	{ value: "sessions", label: "Sessions" },
	{ value: "people", label: "People" },
] as const;

type View = (typeof VIEWS)[number]["value"];

export function WorkPane({
	transcript,
	organization,
	workspace,
	record,
}: {
	transcript: TranscriptSnapshot;
	organization: string;
	workspace: string;
	record: Workspace | undefined;
}) {
	const [view, setView] = useState<View>("work");

	return (
		<>
			<PaneHeading side="work">Work</PaneHeading>
			<Tabs
				value={view}
				onValueChange={(value: View) => setView(value)}
				className="relative flex min-h-0 flex-1 flex-col gap-0"
			>
				<TabsList className="w-full shrink-0 justify-start" aria-label="Work views">
					{VIEWS.map(({ value, label }) => (
						<TabsTrigger key={value} value={value}>
							{label}
						</TabsTrigger>
					))}
				</TabsList>
				<ComposerSkipLink name="Go to the composer" />
				<TabsContent value="work" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<WorkTab organization={organization} workspace={workspace} record={record} />
				</TabsContent>
				<TabsContent value="changes" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<ChangesTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="files" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<FilesTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="sessions" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<SessionsTab organization={organization} workspace={workspace} />
				</TabsContent>
				<TabsContent value="people" keepMounted className="min-h-0 flex-1 overflow-y-auto">
					<PeopleTab transcript={transcript} />
				</TabsContent>
			</Tabs>
		</>
	);
}
