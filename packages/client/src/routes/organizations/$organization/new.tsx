import { createFileRoute } from "@tanstack/react-router";
import { NewWorkspaceForm } from "#/components/workbench/new-workspace-form";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";

export const Route = createFileRoute("/organizations/$organization/new")({
	component: NewWorkspace,
});

function NewWorkspace() {
	const { organization } = Route.useParams();

	return (
		<Workbench
			workspaces={<WorkspacesPane organization={organization} />}
			transcript={<NewWorkspaceForm organization={organization} />}
			work={<PaneHeading>Work</PaneHeading>}
		/>
	);
}
