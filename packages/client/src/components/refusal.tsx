import { CircleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "#/components/ui/alert";
import { Refused, Unreachable } from "#/operator/transport";

export function Refusal({ error }: { error: unknown }) {
	if (error instanceof Refused) {
		return (
			<Alert variant="destructive">
				<CircleAlert aria-hidden />
				<AlertTitle className="line-clamp-none">{error.message}</AlertTitle>
				<AlertDescription>
					<dl className="grid grid-cols-[auto_1fr] gap-x-3">
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
			</Alert>
		);
	}

	return (
		<Alert variant="destructive">
			<CircleAlert aria-hidden />
			<AlertTitle className="line-clamp-none">
				{error instanceof Unreachable ? error.message : "The browser Client failed"}
			</AlertTitle>
		</Alert>
	);
}
