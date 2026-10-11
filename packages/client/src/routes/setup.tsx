import { createFileRoute } from "@tanstack/react-router";
import { Setup } from "#/components/setup";

export const Route = createFileRoute("/setup")({
	validateSearch: (search: Record<string, unknown>): { organization?: string } => ({
		organization: typeof search.organization === "string" ? search.organization : undefined,
	}),
	component: SetupRoute,
});

function SetupRoute() {
	const { organization } = Route.useSearch();
	return <Setup organization={organization} />;
}
