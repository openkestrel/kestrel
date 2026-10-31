import { createFileRoute } from "@tanstack/react-router";
import { GithubSetup } from "#/components/settings/github";

// Not nested under /setup: the operator returns here from GitHub with the flow's safe reference.
export const Route = createFileRoute("/setup_/github")({
	validateSearch: (
		search: Record<string, unknown>,
	): { organization?: string; github_app_flow?: string } => ({
		organization: typeof search.organization === "string" ? search.organization : undefined,
		github_app_flow:
			typeof search.github_app_flow === "string" ? search.github_app_flow : undefined,
	}),
	component: GithubSetupRoute,
});

function GithubSetupRoute() {
	const { organization, github_app_flow } = Route.useSearch();
	return <GithubSetup organization={organization} flow={github_app_flow} />;
}
