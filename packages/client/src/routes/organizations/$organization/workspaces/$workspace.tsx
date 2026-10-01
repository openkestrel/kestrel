import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { SessionQueueLine } from "#/components/workbench/session-queue-line";
import { SessionStatus } from "#/components/workbench/session-status";
import { TranscriptPane } from "#/components/workbench/transcript-pane";
import { WorkPane } from "#/components/workbench/work-pane";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";
import {
	sessionQuery,
	workspaceQuery,
	workspaceSessionsQuery,
	workspacesQuery,
} from "#/operator/queries";

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
	const sessions = useQuery(workspaceSessionsQuery(organization, workspace));
	const known = useQuery(workspacesQuery(organization));
	const current = shown.data?.unfinished_session?.id ?? sessions.data?.at(-1)?.id;
	const session = useQuery({
		...sessionQuery(organization, current ?? ""),
		enabled: current !== undefined,
	});

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
						<>
							<SessionQueueLine organization={organization} workspace={shown.data.id} />
							<SessionStatus organization={organization} record={shown.data} />
							<TranscriptPane
								currency={currency}
								organization={organization}
								read={shown.data}
								session={session.data}
								workspace={workspace}
								workspaces={known.data}
							/>
						</>
					)}
				</>
			}
			work={
				<WorkPane
					currency={currency}
					organization={organization}
					workspace={workspace}
					record={shown.data}
				/>
			}
		/>
	);
}
