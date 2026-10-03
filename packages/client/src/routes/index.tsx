import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, Navigate } from "@tanstack/react-router";
import { Refusal } from "#/components/refusal";
import { Skeleton } from "#/components/ui/skeleton";
import { organizationsQuery } from "#/operator/queries";

export const Route = createFileRoute("/")({
	component: Organizations,
});

function Organizations() {
	const organizations = useQuery(organizationsQuery);

	if (organizations.data?.length === 1) {
		return (
			<Navigate
				to="/organizations/$organization"
				params={{ organization: organizations.data[0].name }}
				replace
			/>
		);
	}

	return (
		<main className="mx-auto grid max-w-xl gap-4 p-8">
			<h1 className="font-semibold text-lg">Organizations</h1>
			{organizations.isPending ? (
				<Skeleton className="h-8 w-full" />
			) : organizations.isError ? (
				<Refusal error={organizations.error} />
			) : organizations.data.length === 0 ? (
				<p className="text-muted-foreground text-sm">
					No Organization exists yet. <code>kestrel start</code> declares one.
				</p>
			) : (
				<ul className="grid gap-1">
					{organizations.data.map(({ id, name }) => (
						<li key={id}>
							<Link
								to="/organizations/$organization"
								params={{ organization: name }}
								className="underline"
							>
								{name}
							</Link>
						</li>
					))}
				</ul>
			)}
		</main>
	);
}
