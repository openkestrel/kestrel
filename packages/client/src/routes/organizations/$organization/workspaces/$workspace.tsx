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
import { useTranscript } from "#/operator/currency";
import type { Entry } from "#/operator/generated";
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
	const { entries } = useTranscript(currency, organization, workspace);

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
								{entries.length === 0 ? (
									<ConversationEmptyState
										title="Transcript"
										description={`${shown.data.project} on ${shown.data.checkout.branch}`}
									/>
								) : (
									entries.map((entry) => (
										<p key={entry.seq} data-seq={entry.seq} className="px-2 py-1 text-sm">
											{spoken(entry.entry)}
										</p>
									))
								)}
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

function spoken(entry: Entry): string {
	switch (entry.type) {
		case "said":
			return `${entry.participant}: ${entry.message}`;
		case "brief":
			return typeof entry.brief === "string" ? `Brief: ${entry.brief}` : "Brief";
		case "participant_joined":
			return `${entry.participant} joined`;
		case "session_started":
			return `${entry.agent} started`;
		case "session_ended":
			return "Session ended";
		case "thought":
			return "Thought";
		case "plan":
			return "Plan";
		case "messages":
			return "Messages";
		case "pull_request":
			return "Pull request";
		case "tool_call":
			return entry.title;
		case "instance_released":
			return "Instance released";
		default: {
			const unhandled: never = entry;
			throw new Error(`no such Transcript entry: ${String(unhandled)}`);
		}
	}
}
