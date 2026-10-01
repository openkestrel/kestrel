import { useQuery } from "@tanstack/react-query";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import type { Workspace } from "#/operator/generated";
import { workQuery } from "#/operator/queries";
import {
	changesText,
	commitsText,
	pushedText,
	repositoryName,
	stashedText,
	untrackedText,
} from "#/operator/work-view";
import { learnedPullRequest, pullRequestsUnavailable, when } from "#/operator/workspace-list";

export function WorkTab({
	organization,
	workspace,
	record,
}: {
	organization: string;
	workspace: string;
	record: Workspace | undefined;
}) {
	const work = useQuery(workQuery(organization, workspace));
	const pull = record ? learnedPullRequest(record) : undefined;
	const unavailable = record ? pullRequestsUnavailable(record) : false;

	if (work.isPending) return <Skeleton className="m-4 h-8" />;
	if (work.isError) {
		return (
			<div className="p-4">
				<Refusal error={work.error} />
			</div>
		);
	}

	const summary = work.data;

	return (
		<div className="grid gap-4 p-4 text-sm">
			<div className="grid gap-1">
				<span className="text-muted-foreground text-xs">
					{record ? `declared branch ${record.checkout.branch}` : "declared branch unknown"}
				</span>
				<span data-pull-request>
					{pull
						? `pull request #${pull.number} ${pull.state}: ${pull.title}`
						: unavailable
							? "pull requests unavailable"
							: "no pull request learned"}
				</span>
			</div>

			{summary.state === "no_instance" ? (
				<p data-work-unavailable>
					No Instance is running, so there is no live work reading. Its work is on the branch{" "}
					{summary.branch}.
				</p>
			) : summary.state === "not_answering" ? (
				<p data-work-unavailable>{summary.message}</p>
			) : (
				<>
					<span className="text-muted-foreground text-xs" data-reported>
						reported {when(summary.reported_at)}
					</span>
					{summary.repositories.map((repository) => (
						<article key={repository.repository} className="grid gap-2 rounded-md border p-3">
							<header className="flex flex-wrap items-baseline gap-2">
								<h3 className="font-medium">{repositoryName(repository)}</h3>
							</header>
							{repository.git === "unreadable" ? (
								<p className="text-muted-foreground text-xs">{repository.because}</p>
							) : (
								<dl className="grid grid-cols-key-value gap-x-3 gap-y-1">
									<dt className="text-muted-foreground">Branch</dt>
									<dd data-work="branch">{repository.branch ?? "detached"}</dd>
									<dt className="text-muted-foreground">Pushed</dt>
									<dd data-work="pushed">{pushedText(repository.pushed)}</dd>
									<dt className="text-muted-foreground">Committed</dt>
									<dd data-work="committed">{commitsText(repository.committed)}</dd>
									<dt className="text-muted-foreground">Staged</dt>
									<dd data-work="staged">{changesText(repository.staged)}</dd>
									<dt className="text-muted-foreground">Changed</dt>
									<dd data-work="changed">{changesText(repository.changed)}</dd>
									<dt className="text-muted-foreground">Untracked</dt>
									<dd data-work="untracked">{untrackedText(repository.untracked)}</dd>
									<dt className="text-muted-foreground">Stashed</dt>
									<dd data-work="stashed">{stashedText(repository.stashed)}</dd>
								</dl>
							)}
						</article>
					))}
				</>
			)}
		</div>
	);
}
