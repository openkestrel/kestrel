import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { Diagnosis, Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { QueuedSessions } from "#/components/workbench/queue-read";
import { useNow } from "#/lib/use-now";
import { ago, leaseLine, moment } from "#/operator/format";
import { sessionsKey, workspaceSessionsQuery } from "#/operator/queries";
import { sessionPhase } from "#/operator/session-state";
import { optionSummary, sessionContinuity, sessionOutcome, usageText } from "#/operator/work-view";

export function SessionsTab({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	return (
		<div className="grid gap-3 p-4 text-sm">
			<QueuedSessions organization={organization} />
			<WorkspaceSessions organization={organization} workspace={workspace} />
		</div>
	);
}

function WorkspaceSessions({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const sessions = useQuery(workspaceSessionsQuery(organization, workspace));

	if (sessions.isPending) return <Skeleton className="h-8" />;
	if (sessions.isError) {
		return <Refusal error={sessions.error} retry={() => void sessions.refetch()} />;
	}
	if (sessions.data.length === 0) {
		return <p className="text-muted-foreground">This Workspace has no Session.</p>;
	}

	return (
		<div className="grid gap-3">
			{sessions.data.toReversed().map((session) => {
				const outcome = sessionOutcome(session);
				const options = optionSummary(session);
				const usage = usageText(session);

				return (
					<article
						key={session.id}
						data-session={session.name}
						className="grid gap-2 rounded-md border p-3"
					>
						<header className="flex flex-wrap items-baseline gap-2">
							<h3 className="font-medium">{session.name}</h3>
							<span className="text-muted-foreground text-xs">{sessionPhase(session)}</span>
							{session.title !== null && (
								<span className="truncate text-muted-foreground text-xs">“{session.title}”</span>
							)}
						</header>
						<dl className="grid grid-cols-key-value gap-x-3 gap-y-1">
							<dt className="text-muted-foreground">Agent</dt>
							<dd data-session-agent>
								{session.agent} · {session.harness}
							</dd>
							<dt className="text-muted-foreground">Model</dt>
							<dd data-session-model>
								requested {session.model ?? "harness default"} · effective{" "}
								{session.worked_model ?? "unknown"}
							</dd>
							{outcome !== undefined && (
								<>
									<dt className="text-muted-foreground">Outcome</dt>
									<dd data-session-outcome>{outcome}</dd>
								</>
							)}
							<dt className="text-muted-foreground">Continuity</dt>
							<dd data-session-continuity>{sessionContinuity(session)}</dd>
							{session.lease_expires_at !== null && (
								<>
									<dt className="text-muted-foreground">Lease</dt>
									<Lease organization={organization} deadline={session.lease_expires_at} />
								</>
							)}
							{options.length > 0 && (
								<>
									<dt className="text-muted-foreground">Options</dt>
									<dd data-session-options>{options.join(" · ")}</dd>
								</>
							)}
							{session.commands.length > 0 && (
								<>
									<dt className="text-muted-foreground">Commands</dt>
									<dd data-session-commands>
										{session.commands.map((command) => command.name).join(", ")}
									</dd>
								</>
							)}
							{usage !== undefined && (
								<>
									<dt className="text-muted-foreground">Usage</dt>
									<dd>{usage}</dd>
								</>
							)}
							<dt className="text-muted-foreground">Enqueued</dt>
							<dd>{ago(session.enqueued_at)}</dd>
						</dl>
						{session.diagnostic && <Diagnosis diagnostic={session.diagnostic} />}
					</article>
				);
			})}
		</div>
	);
}

const REREAD_EVERY = 15_000;

// Only the control plane judges a lease expired, so a passed deadline re-reads the Sessions and ends nothing here.
function Lease({ organization, deadline }: { organization: string; deadline: string }) {
	const queryClient = useQueryClient();
	const now = useNow(5_000);
	const passed = Date.parse(deadline) <= now;

	useEffect(() => {
		if (!passed) return undefined;
		const reread = () =>
			void queryClient.invalidateQueries({ queryKey: sessionsKey(organization) });
		reread();
		const again = setInterval(reread, REREAD_EVERY);
		return () => clearInterval(again);
	}, [passed, organization, queryClient]);

	return (
		<dd data-session-lease={passed ? "expired" : "ahead"}>
			{leaseLine(deadline, now)} · <time dateTime={deadline}>{moment(deadline)}</time>
		</dd>
	);
}
