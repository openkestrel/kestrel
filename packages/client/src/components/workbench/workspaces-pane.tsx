import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { Refusal } from "#/components/refusal";
import { buttonVariants } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import type { Queue } from "#/operator/generated";
import { queueQuery, workspacesQuery } from "#/operator/queries";
import { instancesLine, slotsLine } from "#/operator/queue-limits";
import { order } from "#/operator/workspace-list";
import { PaneHeading } from "./workbench";
import { WorkspaceRowView } from "./workspace-row";

export function WorkspacesPane({
	organization,
	headingLevel = 2,
}: {
	organization: string;
	headingLevel?: 1 | 2;
}) {
	const workspaces = useQuery(workspacesQuery(organization));
	const queue = useQuery(queueQuery(organization));
	const rows = order(workspaces.data ?? []);

	return (
		<>
			<PaneHeading level={headingLevel}>Workspaces</PaneHeading>
			<QueueHeader queue={queue.data} />
			<div className="p-2">
				<Link
					to="/organizations/$organization/new"
					params={{ organization }}
					className={buttonVariants({ variant: "outline", size: "sm", className: "w-full" })}
				>
					<Plus aria-hidden />
					New Workspace
				</Link>
			</div>
			<nav aria-label="Workspaces" className="min-h-0 flex-1 overflow-y-auto p-2">
				{workspaces.isPending ? (
					<Skeleton className="h-8 w-full" />
				) : workspaces.isError ? (
					<Refusal error={workspaces.error} />
				) : rows.length === 0 ? (
					<p className="px-2 text-muted-foreground text-sm">No Workspace is open.</p>
				) : (
					<ul className="grid gap-1">
						{rows.map((row) => (
							<li key={row.id}>
								<WorkspaceRowView organization={organization} row={row} />
							</li>
						))}
					</ul>
				)}
			</nav>
		</>
	);
}

function QueueHeader({ queue }: { queue: Queue | undefined }) {
	if (!queue) return null;

	return (
		<div
			data-queue-header
			className="flex flex-wrap gap-x-3 gap-y-0.5 border-b px-3 py-2 text-muted-foreground text-xs"
		>
			<span data-slots className="min-w-0 wrap-break-word">
				{slotsLine(queue)}
			</span>
			<span data-instances className="min-w-0 wrap-break-word">
				{instancesLine(queue)}
			</span>
			{queue.work_role && <span data-driver>{queue.work_role.driver}</span>}
		</div>
	);
}
