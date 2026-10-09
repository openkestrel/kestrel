import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
	Queue,
	QueueItem,
	QueueItemContent,
	QueueItemDescription,
	QueueItemIndicator,
	QueueList,
	QueueSection,
	QueueSectionContent,
	QueueSectionLabel,
	QueueSectionTrigger,
} from "#/components/ai-elements/queue";
import { Refusal, RetryFallback } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { ago } from "#/operator/format";
import { queueQuery } from "#/operator/queries";
import { knownQueue, type QueueRead, queuedSessions, queueRead } from "#/operator/queue-read";
import { diagnosisOf } from "#/operator/transport";

const DELAYED_AFTER_MS = 2_000;

export type QueueReading = { read: QueueRead; retry: () => void; readAt: number };

export function useQueueRead(organization: string): QueueReading {
	const queue = useQuery(queueQuery(organization));
	const [delayedFor, setDelayedFor] = useState<string>();

	useEffect(() => {
		if (!queue.isPending) return undefined;
		const timer = setTimeout(() => setDelayedFor(organization), DELAYED_AFTER_MS);
		return () => clearTimeout(timer);
	}, [queue.isPending, organization]);
	const delayed = delayedFor === organization;

	return {
		read: queueRead({ data: queue.data, error: queue.error, fetching: queue.isFetching }, delayed),
		retry: () => void queue.refetch(),
		readAt: queue.dataUpdatedAt,
	};
}

// Compact for a header strip: one line, no alert, so one failure never stacks several alerts.
export function QueueReadNotice({
	read,
	retry,
	readAt,
	compact = false,
}: QueueReading & { compact?: boolean }) {
	switch (read.kind) {
		case "read":
			return null;
		case "reading":
			if (compact && !read.delayed) return null;
			return (
				<output data-queue-read={read.delayed ? "delayed" : "reading"} className="text-xs">
					{read.delayed
						? "Still reading the queue; the Operator has not answered yet."
						: "Reading the queue…"}
				</output>
			);
		case "failed": {
			if (compact) {
				return (
					<div data-queue-read="failed" className="flex flex-wrap items-center gap-x-2 text-xs">
						<output className="text-destructive">
							The queue could not be read: {diagnosisOf(read.error).message}.
						</output>
						<Button
							size="xs"
							type="button"
							variant="outline"
							disabled={read.retrying}
							onClick={retry}
						>
							Read the queue again
						</Button>
					</div>
				);
			}
			return (
				<div data-queue-read="failed" className="grid gap-1 text-xs">
					<Refusal error={read.error} retry={retry} />
					<RetryFallback
						error={read.error}
						retry={retry}
						label="Read the queue again"
						disabled={read.retrying}
					/>
					<output className="text-muted-foreground">
						{read.retrying
							? "Reading the queue again…"
							: read.known
								? `Showing the queue as last read ${ago(new Date(readAt).toISOString())}.`
								: "The queue has not been read."}
					</output>
				</div>
			);
		}
		default: {
			const unhandled: never = read;
			throw new Error(`no such queue read: ${JSON.stringify(unhandled)}`);
		}
	}
}

export function QueuedSessions({ organization }: { organization: string }) {
	const reading = useQueueRead(organization);
	const { read } = reading;
	const queue = knownQueue(read);
	const rows = queue ? queuedSessions(queue) : [];

	return (
		<section aria-label="Queued Sessions" className="grid gap-2">
			<QueueReadNotice {...reading} />
			{queue &&
				(rows.length === 0 ? (
					<p className="text-muted-foreground">No Session is queued.</p>
				) : (
					<Queue>
						<QueueSection>
							<QueueSectionTrigger>
								<QueueSectionLabel label="queued Sessions" count={rows.length} />
							</QueueSectionTrigger>
							<QueueSectionContent>
								<QueueList>
									{rows.map((row) => (
										<QueueItem key={row.name} data-queued={row.name}>
											<div className="flex items-center gap-2">
												<QueueItemIndicator />
												<QueueItemContent>{row.name}</QueueItemContent>
											</div>
											<QueueItemDescription>
												{row.label}
												{row.position !== null && ` #${row.position}`}
												{row.reasons !== "" && ` · ${row.reasons}`}
											</QueueItemDescription>
										</QueueItem>
									))}
								</QueueList>
							</QueueSectionContent>
						</QueueSection>
					</Queue>
				))}
		</section>
	);
}
