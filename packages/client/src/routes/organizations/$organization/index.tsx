import { createFileRoute } from "@tanstack/react-router";
import { ConversationEmptyState } from "#/components/ai-elements/conversation";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";

export const Route = createFileRoute("/organizations/$organization/")({
	component: Organization,
});

function Organization() {
	const { organization } = Route.useParams();

	return (
		<Workbench
			initial="workspaces"
			workspaces={<WorkspacesPane organization={organization} />}
			transcript={
				<>
					<PaneHeading>Transcript</PaneHeading>
					<ConversationEmptyState
						title="No Workspace is chosen"
						description="Choose a Workspace to follow its Transcript."
					/>
				</>
			}
			work={<PaneHeading>Work</PaneHeading>}
		/>
	);
}
