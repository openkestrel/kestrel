import { createFileRoute } from "@tanstack/react-router";
import { RepositoriesSettings } from "#/components/settings/repositories";

export const Route = createFileRoute("/organizations/$organization/settings/repositories")({
	component: Repositories,
});

function Repositories() {
	const { organization } = Route.useParams();
	return <RepositoriesSettings organization={organization} />;
}
