import { QueueReadNotice, useQueueRead } from "#/components/workbench/queue-read";
import { sessionQueueLine } from "#/operator/queue-line";
import { knownQueue } from "#/operator/queue-read";

export function SessionQueueLine({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const reading = useQueueRead(organization);
	const queue = knownQueue(reading.read);
	const line = queue ? sessionQueueLine(queue, workspace) : undefined;

	return (
		<div
			data-queue-line
			className="grid shrink-0 gap-1 border-b px-4 py-2 text-muted-foreground text-xs empty:hidden"
		>
			{line && <output>{line}</output>}
			<QueueReadNotice {...reading} compact />
		</div>
	);
}
