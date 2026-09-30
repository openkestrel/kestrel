import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import { workspacesQuery } from "#/operator/queries";
import { PaneHeading } from "./workbench";

export function WorkspacesPane({ organization }: { organization: string }) {
	const workspaces = useQuery(workspacesQuery(organization));

	return (
		<>
			<PaneHeading>Workspaces</PaneHeading>
			<div className="p-2">
				<Button asChild variant="outline" size="sm" className="w-full">
					<Link to="/organizations/$organization/new" params={{ organization }}>
						<Plus aria-hidden />
						New Workspace
					</Link>
				</Button>
			</div>
			<nav aria-label="Workspaces" className="min-h-0 flex-1 overflow-y-auto p-2">
				{workspaces.isPending ? (
					<Skeleton className="h-8 w-full" />
				) : workspaces.isError ? (
					<Refusal error={workspaces.error} />
				) : workspaces.data.length === 0 ? (
					<p className="px-2 text-muted-foreground text-sm">No Workspace is open.</p>
				) : (
					<ul className="grid gap-1">
						{workspaces.data.map((workspace) => (
							<li key={workspace.id}>
								<Link
									to="/organizations/$organization/workspaces/$workspace"
									params={{ organization, workspace: workspace.name }}
									className="block truncate rounded-md px-2 py-1.5 text-sm hover:bg-accent aria-[current=page]:bg-accent aria-[current=page]:font-medium"
								>
									{workspace.name}
								</Link>
							</li>
						))}
					</ul>
				)}
			</nav>
		</>
	);
}
