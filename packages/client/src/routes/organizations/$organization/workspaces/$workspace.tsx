import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { TranscriptPane } from "#/components/workbench/transcript-pane";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";
import { workspaceQuery } from "#/operator/queries";

export const Route = createFileRoute("/organizations/$organization/workspaces/$workspace")({
	component: WorkspaceView,
	beforeLoad: async ({ context, params, preload }) => {
		context.currency.watch(params.organization);
		if (!preload) await context.currency.ready(params.organization);
	},
});

function WorkspaceView() {
	const { organization, workspace } = Route.useParams();
	const { currency } = Route.useRouteContext();
	const shown = useQuery(workspaceQuery(organization, workspace));

	return (
		<Workbench
			workspaces={<WorkspacesPane organization={organization} />}
			transcript={
				<>
					<PaneHeading>{shown.data?.name ?? workspace}</PaneHeading>
					{shown.isPending ? (
						<Skeleton className="m-4 h-8" />
					) : shown.isError ? (
						<div className="p-4">
							<Refusal error={shown.error} />
						</div>
					) : (
						<TranscriptPane
							currency={currency}
							organization={organization}
							workspace={workspace}
							empty={{ project: shown.data.project, branch: shown.data.checkout.branch }}
						/>
					)}
				</>
			}
			work={<PaneHeading>Work</PaneHeading>}
		/>
	);
}
