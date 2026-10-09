import { Button } from "#/components/ui/button";
import { useQueueRead } from "#/components/workbench/queue-read";
import { sessionQueueLine } from "#/operator/queue-line";
import { knownQueue } from "#/operator/queue-read";
import { diagnosisOf } from "#/operator/transport";

export function SessionQueueLine({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const { read, retry } = useQueueRead(organization);
	const queue = knownQueue(read);
	const line = queue ? sessionQueueLine(queue, workspace) : undefined;
	const failure =
		read.kind === "failed"
			? `The queue could not be read: ${diagnosisOf(read.error).message}`
			: undefined;
	const delayed = read.kind === "reading" && read.delayed;
	if (!line && !failure && !delayed) return null;

	return (
		<div
			data-queue-line
			data-queue-read={read.kind === "failed" ? "failed" : undefined}
			className="flex shrink-0 flex-wrap items-center gap-x-2 border-b px-4 py-2 text-muted-foreground text-xs"
		>
			<output>
				{delayed && "Still reading the queue; the Operator has not answered yet."}
				{line}
				{failure && <span className="block text-destructive">{failure}</span>}
			</output>
			{read.kind === "failed" && (
				<Button size="xs" type="button" variant="outline" disabled={read.retrying} onClick={retry}>
					{read.retrying ? "Reading the queue again…" : "Read the queue again"}
				</Button>
			)}
		</div>
	);
}
