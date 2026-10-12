import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Button } from "#/components/ui/button";
import { AN_EMPTY_DRAFT, holdDraft } from "#/lib/new-workspace-draft";
import { useNow } from "#/lib/use-now";
import { ago, absoluteTime } from "#/operator/format";
import type { TranscriptSessionState, Workspace } from "#/operator/generated";
import { continueDraft, sessionStatusLine } from "#/operator/getting-ready";
import { workspaceSessionsQuery } from "#/operator/queries";
import { type OpenWork, type Trailing, trailingOf } from "#/operator/session-view";

export function SessionStatus({
	organization,
	record,
	live,
}: {
	organization: string;
	record: Workspace | undefined;
	live: TranscriptSessionState | undefined;
}) {
	const navigate = useNavigate();
	const sessions = useQuery({
		...workspaceSessionsQuery(organization, record?.name ?? ""),
		enabled: record !== undefined,
	});

	if (!record) return null;

	if (record.state === "sealed") {
		return (
			<div className="shrink-0 border-b px-4 py-2">
				<Button
					type="button"
					variant="outline"
					size="sm"
					onClick={() => {
						holdDraft(organization, { ...AN_EMPTY_DRAFT, ...continueDraft(record) });
						void navigate({
							to: "/organizations/$organization/new",
							params: { organization },
						});
					}}
				>
					Continue in a new Workspace
				</Button>
			</div>
		);
	}

	const session = sessions.data?.at(-1);
	const trailing = trailingOf(session, live);
	if (trailing) return <TrailingWork trailing={trailing} />;

	const line = sessionStatusLine(session);
	if (line === undefined) return null;

	return (
		<output
			data-preparing
			aria-live="polite"
			className="block shrink-0 border-b px-4 py-2 text-muted-foreground text-xs"
		>
			{line}
		</output>
	);
}

function TrailingWork({ trailing }: { trailing: Trailing }) {
	const now = useNow(15_000);

	return (
		<section
			aria-label="Trailing work"
			data-trailing={trailing.observed}
			className="grid shrink-0 gap-1 border-b px-4 py-2 text-muted-foreground text-xs"
		>
			<output aria-live="polite">
				{trailing.observed === "open"
					? "The agent answered, and work it started is still open."
					: trailing.observed === "settling"
						? "Checking that background work has settled."
						: "Open work can’t be observed right now."}{" "}
				You can post a new Turn now.
			</output>
			{trailing.observed === "open" && <WorkList label="Open work" work={trailing.work} />}
			{trailing.observed === "unavailable" && trailing.last && (
				<div data-trailing-last className="grid gap-1">
					<p>
						Last observed {ago(trailing.last.observedAt, now)} ·{" "}
						<time dateTime={trailing.last.observedAt}>
							{absoluteTime(trailing.last.observedAt)}
						</time>
						. This is history, not what is open now.
					</p>
					{trailing.last.work.length === 0 ? (
						<p>Nothing was open then.</p>
					) : (
						<WorkList label="Work open when last observed" work={trailing.last.work} />
					)}
				</div>
			)}
		</section>
	);
}

function WorkList({ label, work }: { label: string; work: OpenWork[] }) {
	return (
		<ul aria-label={label} className="grid gap-0.5">
			{work.map((open) => (
				<li key={open.key} className="flex flex-wrap items-baseline gap-x-2">
					<span className="text-foreground">{open.title}</span>
					<span>{open.kind}</span>
				</li>
			))}
		</ul>
	);
}
