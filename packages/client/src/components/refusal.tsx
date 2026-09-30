import { CircleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "#/components/ui/alert";
import { Refused, Unreachable } from "#/operator/transport";

export function Refusal({ error }: { error: unknown }) {
	const said =
		error instanceof Refused || error instanceof Unreachable
			? error.message
			: "The browser Client failed";

	return (
		<Alert variant="destructive">
			<CircleAlert aria-hidden />
			<AlertTitle>{said}</AlertTitle>
			{error instanceof Refused && (
				<AlertDescription>
					<dl className="grid grid-cols-key-value gap-x-3">
						<dt>Status</dt>
						<dd>{error.status}</dd>
						{error.field && (
							<>
								<dt>Field</dt>
								<dd>
									<code>{error.field}</code>
								</dd>
							</>
						)}
						{error.phase && (
							<>
								<dt>Phase</dt>
								<dd>{error.phase}</dd>
							</>
						)}
					</dl>
				</AlertDescription>
			)}
		</Alert>
	);
}
