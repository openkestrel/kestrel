import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Button } from "#/components/ui/button";
import { AN_EMPTY_DRAFT, holdDraft } from "#/lib/new-workspace-draft";
import type { Workspace } from "#/operator/generated";
import { continueDraft, sessionStatusLine } from "#/operator/getting-ready";
import { workspaceSessionsQuery } from "#/operator/queries";

export function SessionStatus({
	organization,
	record,
}: {
	organization: string;
	record: Workspace | undefined;
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

	const line = sessionStatusLine(sessions.data?.at(-1));
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
