import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { CircleAlert } from "lucide-react";
import { workQuery } from "#/operator/queries";
import { currentUnit, needsAttention, phaseLabel } from "#/operator/session-state";
import {
	changedWork,
	learnedPullRequest,
	pullRequestsUnavailable,
	type WorkspaceRow,
	waitingText,
	workNote,
} from "#/operator/workspace-list";

export function WorkspaceRowView({
	organization,
	row,
}: {
	organization: string;
	row: WorkspaceRow;
}) {
	const work = useQuery(workQuery(organization, row.workspace.name));
	const attention = needsAttention(row);
	const changed = changedWork(work.data);
	const pull = learnedPullRequest(row.workspace);
	const unit = currentUnit(row.session);
	const waiting = waitingText(row);
	const note = workNote(work.data);

	return (
		<Link
			to="/organizations/$organization/workspaces/$workspace"
			params={{ organization, workspace: row.workspace.name }}
			data-attention={attention ? "true" : "false"}
			className="block rounded-md px-2 py-1.5 text-sm hover:bg-accent aria-[current=page]:bg-accent aria-[current=page]:font-medium"
		>
			<span className="flex items-center gap-1.5">
				{attention && (
					<>
						<CircleAlert aria-hidden className="size-3.5 shrink-0 text-destructive" />
						<span className="sr-only">Needs attention</span>
					</>
				)}
				<span className="truncate font-medium">{row.session?.title ?? row.workspace.name}</span>
				<span className="ml-auto shrink-0 text-muted-foreground text-xs">{phaseLabel(row)}</span>
			</span>
			{unit && <span className="block truncate text-muted-foreground text-xs">{unit}</span>}
			<span className="mt-0.5 flex flex-wrap gap-x-2 text-muted-foreground text-xs">
				<span className="truncate">{row.workspace.checkout.branch}</span>
				{changed && (
					<>
						<span data-changed>
							+{changed.added} −{changed.removed}
						</span>
						{changed.untracked > 0 && <span data-untracked>{changed.untracked} untracked</span>}
					</>
				)}
				{pull ? (
					<span data-pull-request>
						#{pull.number} {pull.state}
					</span>
				) : (
					pullRequestsUnavailable(row.workspace) && <span>pull requests unavailable</span>
				)}
			</span>
			{waiting && (
				<span data-waiting className="block text-muted-foreground text-xs">
					{waiting}
				</span>
			)}
			{note && (
				<span data-work-note className="block text-muted-foreground text-xs">
					{note}
				</span>
			)}
		</Link>
	);
}
