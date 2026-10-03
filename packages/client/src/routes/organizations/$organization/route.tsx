import { createFileRoute, Outlet } from "@tanstack/react-router";
import { useChangeNotices } from "#/operator/follow";

export const Route = createFileRoute("/organizations/$organization")({
	component: Organization,
});

function Organization() {
	const { organization } = Route.useParams();
	useChangeNotices(organization);
	return <Outlet />;
}
