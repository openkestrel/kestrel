import { useQuery } from "@tanstack/react-query";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { ago } from "#/operator/format";
import { workspaceSessionsQuery } from "#/operator/queries";
import { sessionPhase } from "#/operator/session-state";
import { optionSummary, sessionContinuity, sessionOutcome, usageText } from "#/operator/work-view";

export function SessionsTab({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const sessions = useQuery(workspaceSessionsQuery(organization, workspace));

	if (sessions.isPending) return <Skeleton className="m-4 h-8" />;
	if (sessions.isError) {
		return (
			<div className="p-4">
				<Refusal error={sessions.error} />
			</div>
		);
	}
	if (sessions.data.length === 0) {
		return <p className="p-4 text-muted-foreground text-sm">This Workspace has no Session.</p>;
	}

	return (
		<div className="grid gap-3 p-4 text-sm">
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
					</article>
				);
			})}
		</div>
	);
}
