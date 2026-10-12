import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Shimmer } from "#/components/ai-elements/shimmer";
import { Button } from "#/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { Skeleton } from "#/components/ui/skeleton";
import { cn } from "#/lib/utils";
import type { Activity, RunningTool, TranscriptSessionState } from "#/operator/generated";
import { transcriptRangeQuery } from "#/operator/queries";
import type { Delivered } from "#/operator/transcript";
import {
	activityRead,
	activitySteps,
	activitySummary,
	toolState,
} from "#/operator/transcript-view";
import { diagnosisOf } from "#/operator/transport";
import { EntryRow } from "./entry-row";
import { type Disclosure, ToolStep } from "./step-row";

const DELAYED_AFTER_MS = 2_000;

// `activity` is undefined while the Session's first calls run and nothing is recorded yet;
// `live` is the Session's state for the one Activity it is still working in.
export function ActivityRow({
	activity,
	live,
	mode,
	override,
	onToggle,
	organization,
	workspace,
}: {
	activity: Activity | undefined;
	live: TranscriptSessionState | undefined;
	mode: Disclosure;
	override: boolean | undefined;
	onToggle: (open: boolean) => void;
	organization: string;
	workspace: string;
}) {
	const open = override ?? (mode !== "line" || live !== undefined);
	const trigger = useRef<HTMLButtonElement>(null);
	const range = useQuery({
		...transcriptRangeQuery(organization, workspace, {
			first: activity?.first_seq ?? 0,
			last: activity?.last_seq ?? 0,
		}),
		enabled: open && activity !== undefined,
	});

	const [known, setKnown] = useState<Delivered[]>([]);
	if (range.data !== undefined && range.data !== known) setKnown(range.data);

	const reading = range.isLoading ? `${activity?.first_seq}:${activity?.last_seq}` : undefined;
	const [delayedFor, setDelayedFor] = useState<string>();
	useEffect(() => {
		if (reading === undefined) return undefined;
		const timer = setTimeout(() => setDelayedFor(reading), DELAYED_AFTER_MS);
		return () => clearTimeout(timer);
	}, [reading]);

	const read = activity
		? activityRead(
				{ data: range.data, error: range.error, fetching: range.isFetching },
				known,
				reading !== undefined && delayedFor === reading,
			)
		: undefined;
	const entries =
		read?.kind === "read" ? read.entries : read && read.kind !== "empty" ? read.known : [];

	const running = useMemo(
		() => (live?.tools ?? []).filter((tool) => toolState(tool.status) === "running"),
		[live],
	);
	const settled = activity?.closed === true && !range.isFetching;
	const steps = activitySteps(entries, useUnrecorded(running, entries, settled));
	const summary = activitySummary(activity, live !== undefined);

	const retry = async () => {
		const again = await range.refetch();
		// The retry control leaves with the failure, so focus returns to the Activity it belonged to.
		if (again.isSuccess) trigger.current?.focus();
	};

	return (
		<div data-activity={activity?.first_seq}>
			<Collapsible open={open} onOpenChange={onToggle}>
				<CollapsibleTrigger ref={trigger} aria-label={summary.name}>
					<span className="flex flex-wrap items-center gap-x-1.5 py-0.5 pr-1.5 text-left text-muted-foreground text-sm">
						<ChevronRight
							aria-hidden
							className={cn("size-3.5 shrink-0 transition-transform", open && "rotate-90")}
						/>
						{live && <Shimmer>Working</Shimmer>}
						{live && summary.counts && <span aria-hidden>·</span>}
						{summary.counts ? <span>{summary.counts}</span> : !live && <span>Activity</span>}
						{summary.outcome && <span className="text-destructive">{summary.outcome}</span>}
						{summary.duration && <span className="text-xs tabular-nums">{summary.duration}</span>}
					</span>
				</CollapsibleTrigger>
				<CollapsibleContent>
					<div className="mt-1 ml-1.5 grid min-w-0 gap-px border-l pl-3">
						{(steps.length > 0 || live?.thought_buffering || live?.message_buffering) && (
							<ul aria-label="Activity steps" className="grid min-w-0 gap-px">
								{steps.map((step) =>
									step.kind === "running" ? (
										<li key={step.key} className="min-w-0">
											<ToolStep
												running={step.tool}
												mode={mode}
												organization={organization}
												workspace={workspace}
											/>
										</li>
									) : step.entry.entry.type === "tool_call" ? (
										<li key={step.key} className="min-w-0" data-seq={step.entry.seq}>
											<ToolStep
												entry={step.entry.entry}
												mode={mode}
												organization={organization}
												workspace={workspace}
											/>
										</li>
									) : (
										<li key={step.key} className="min-w-0">
											<EntryRow
												entry={step.entry}
												mode={mode}
												organization={organization}
												workspace={workspace}
											/>
										</li>
									),
								)}
								{live?.thought_buffering && <Buffering>Thinking…</Buffering>}
								{live?.message_buffering && <Buffering>Receiving a message…</Buffering>}
							</ul>
						)}
						{read?.kind === "reading" &&
							(read.delayed ? (
								<output className="px-1.5 py-1 text-muted-foreground text-xs">
									Still reading the Activity; the Operator has not answered yet.
								</output>
							) : (
								steps.length === 0 && <Skeleton className="h-6 w-full" />
							))}
						{read?.kind === "empty" && steps.length === 0 && (
							<p className="px-1.5 py-1 text-muted-foreground text-xs">
								This Activity has no entries.
							</p>
						)}
						{read?.kind === "failed" && (
							<div role="alert" className="flex flex-wrap items-center gap-x-2 px-1.5 py-1 text-xs">
								<span className="text-destructive">
									The Activity could not be read: {diagnosisOf(read.error).message}.
								</span>
								<Button
									size="xs"
									type="button"
									variant="outline"
									disabled={read.retrying}
									onClick={() => void retry()}
								>
									Read the Activity again
								</Button>
							</div>
						)}
					</div>
				</CollapsibleContent>
			</Collapsible>
		</div>
	);
}

function Buffering({ children }: { children: string }) {
	return (
		<li className="px-1.5 py-1 text-sm">
			<Shimmer>{children}</Shimmer>
		</li>
	);
}

// The Session's state drops a call before the entry that records it is read, so the row is held
// until then: otherwise it would vanish and come back.
function useUnrecorded(
	running: RunningTool[],
	entries: Delivered[],
	settled: boolean,
): RunningTool[] {
	const [held, setHeld] = useState<RunningTool[]>([]);
	const merged = [
		...running,
		...held.filter((tool) => !running.some((other) => other.call_id === tool.call_id)),
	];
	const next = settled
		? []
		: merged.filter(
				(tool) =>
					!entries.some(
						({ entry }) => entry.type === "tool_call" && entry.call_id === tool.call_id,
					),
			);
	const changed = next.length !== held.length || next.some((tool, index) => tool !== held[index]);
	if (changed) setHeld(next);
	return merged;
}
