import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import {
	Conversation,
	ConversationContent,
	ConversationEmptyState,
	ConversationScrollButton,
} from "#/components/ai-elements/conversation";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { PaneHeading, Workbench } from "#/components/workbench/workbench";
import { WorkspacesPane } from "#/components/workbench/workspaces-pane";
import { workspaceQuery } from "#/operator/queries";

export const Route = createFileRoute("/organizations/$organization/workspaces/$workspace")({
	component: WorkspaceView,
});

function WorkspaceView() {
	const { organization, workspace } = Route.useParams();
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
						<Conversation>
							<ConversationContent>
								<ConversationEmptyState
									title="Transcript"
									description={`${shown.data.project} on ${shown.data.checkout.branch}`}
								/>
							</ConversationContent>
							<ConversationScrollButton />
						</Conversation>
					)}
				</>
			}
			work={<PaneHeading>Work</PaneHeading>}
		/>
	);
}
