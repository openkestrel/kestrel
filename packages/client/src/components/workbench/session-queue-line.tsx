import { useQuery } from "@tanstack/react-query";
import { queueQuery } from "#/operator/queries";
import { sessionQueueLine } from "#/operator/queue-line";

export function SessionQueueLine({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const queue = useQuery(queueQuery(organization));
	const line = queue.data ? sessionQueueLine(queue.data, workspace) : undefined;
	if (!line) return null;

	return (
		<output data-live className="block shrink-0 border-b px-4 py-2 text-muted-foreground text-xs">
			{line}
		</output>
	);
}
