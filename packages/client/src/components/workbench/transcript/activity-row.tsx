import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { Skeleton } from "#/components/ui/skeleton";
import { cn } from "#/lib/utils";
import type { Activity } from "#/operator/generated";
import { transcriptRangeQuery } from "#/operator/queries";
import { activityCounts, activityDuration, activityWindow } from "#/operator/transcript-view";
import { EntryRow, type Disclosure } from "./entry-row";

export function ActivityRow({
	activity,
	mode,
	override,
	onToggle,
	organization,
	workspace,
}: {
	activity: Activity;
	mode: Disclosure;
	override: boolean | undefined;
	onToggle: (open: boolean) => void;
	organization: string;
	workspace: string;
}) {
	const open = override ?? mode !== "line";
	const range = useQuery({
		...transcriptRangeQuery(organization, workspace, {
			first: activity.first_seq,
			last: activity.last_seq,
		}),
		enabled: open,
	});
	const counts = activityCounts(activity);
	const duration = activityDuration(activity);

	return (
		<div className="rounded-md border" data-activity={activity.first_seq}>
			<Collapsible open={open} onOpenChange={onToggle}>
				<CollapsibleTrigger className="w-full">
					<span className="flex items-start gap-2 px-2 py-1.5 text-left">
						<ChevronRight
							className={cn("mt-0.5 size-4 shrink-0 transition-transform", open && "rotate-90")}
							aria-hidden
						/>
						<span className="min-w-0 flex-1">
							<span className="flex flex-wrap items-baseline gap-x-2 text-sm">
								<span className="font-medium">Activity {activityWindow(activity)}</span>
								{counts && <span className="text-muted-foreground">{counts}</span>}
								{duration && <span className="text-muted-foreground">{duration}</span>}
								{activity.anomaly && (
									<span className="text-destructive">interrupted or unresolved</span>
								)}
							</span>
							{activity.latest && (
								<span className="mt-0.5 block truncate text-muted-foreground text-xs">
									{activity.latest.title ?? activity.latest.kind}
									{activity.latest.status ? ` · ${activity.latest.status}` : ""}
								</span>
							)}
						</span>
						{!activity.closed && <span className="text-muted-foreground text-xs">open</span>}
					</span>
				</CollapsibleTrigger>
				<CollapsibleContent>
					<div className="border-t px-2 py-1">
						{range.isPending ? (
							<Skeleton className="h-6 w-full" />
						) : range.isError ? (
							<p className="text-destructive text-sm">The Activity could not be read</p>
						) : (
							range.data.map((entry) => (
								<EntryRow
									key={entry.seq}
									entry={entry}
									mode={mode}
									organization={organization}
									workspace={workspace}
								/>
							))
						)}
					</div>
				</CollapsibleContent>
			</Collapsible>
		</div>
	);
}
