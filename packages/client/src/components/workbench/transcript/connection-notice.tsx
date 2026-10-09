import { Refusal, RetryFallback } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import type { Connection } from "#/operator/transcript";
import { diagnosisOf } from "#/operator/transport";

export function ConnectionNotice({
	connection,
	reconnect,
}: {
	connection: Connection;
	reconnect: () => void;
}) {
	switch (connection.state) {
		case "connecting":
		case "live":
			return null;
		case "reconnecting":
			return (
				<div
					data-transcript-connection="reconnecting"
					className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b px-4 py-2 text-xs"
				>
					<output>
						Reconnecting to the Transcript; what is shown stays, and it resumes where it left off.
						{connection.failure !== undefined && (
							<span className="block text-muted-foreground">
								{diagnosisOf(connection.failure).message}
							</span>
						)}
					</output>
					<Button size="xs" type="button" variant="outline" onClick={reconnect}>
						Reconnect now
					</Button>
				</div>
			);
		case "unavailable": {
			return (
				<section
					aria-label="Transcript unavailable"
					data-transcript-connection="unavailable"
					className="grid shrink-0 gap-2 border-b p-4 text-xs"
				>
					<p className="font-medium">The Transcript is unavailable.</p>
					<Refusal error={connection.failure} retry={reconnect} />
					<RetryFallback error={connection.failure} retry={reconnect} label="Reconnect" />
					{connection.retriesItself && (
						<output className="text-muted-foreground">Trying again by itself.</output>
					)}
				</section>
			);
		}
		default: {
			const unhandled: never = connection;
			throw new Error(`no such connection: ${JSON.stringify(unhandled)}`);
		}
	}
}
