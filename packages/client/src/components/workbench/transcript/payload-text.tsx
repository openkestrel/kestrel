import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import type { PayloadReference } from "#/operator/generated";
import { transcriptPayloadQuery } from "#/operator/queries";
import { bytes } from "#/operator/transcript-view";
import { Refused } from "#/operator/transport";

export function PayloadText({
	organization,
	workspace,
	reference,
}: {
	organization: string;
	workspace: string;
	reference: PayloadReference;
}) {
	const [wanted, setWanted] = useState(false);
	const payload = useQuery({
		...transcriptPayloadQuery(organization, workspace, reference.payload_id),
		enabled: wanted,
	});

	if (!wanted) {
		return (
			<Button type="button" variant="outline" size="sm" onClick={() => setWanted(true)}>
				Load {bytes(reference.bytes)} payload
			</Button>
		);
	}

	if (payload.isPending) return <Skeleton className="h-4 w-40" />;

	if (payload.isError) {
		const expired = payload.error instanceof Refused && payload.error.status === 410;
		return (
			<span className="text-muted-foreground text-xs">
				{expired ? "expired" : "the payload could not be read"}
			</span>
		);
	}

	return <pre className="max-h-64 overflow-auto rounded-md border p-2 text-xs">{payload.data}</pre>;
}
