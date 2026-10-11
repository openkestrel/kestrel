import { createFileRoute } from "@tanstack/react-router";
import { AgentsSettings } from "#/components/settings/agents";

export const Route = createFileRoute("/organizations/$organization/settings/agents")({
	component: Agents,
});

function Agents() {
	const { organization } = Route.useParams();
	return <AgentsSettings organization={organization} />;
}
