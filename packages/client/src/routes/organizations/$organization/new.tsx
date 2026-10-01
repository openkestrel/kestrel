import { createFileRoute } from "@tanstack/react-router";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";

export const Route = createFileRoute("/organizations/$organization/new")({
	component: NewWorkspace,
	beforeLoad: async ({ context, params, preload }) => {
		context.currency.watch(params.organization);
		if (!preload) await context.currency.ready(params.organization);
	},
});

function NewWorkspace() {
	const { organization } = Route.useParams();

	return (
		<Workbench
			workspaces={<WorkspacesPane organization={organization} />}
			transcript={<PaneHeading>New Workspace</PaneHeading>}
			work={<PaneHeading>Work</PaneHeading>}
		/>
	);
}
