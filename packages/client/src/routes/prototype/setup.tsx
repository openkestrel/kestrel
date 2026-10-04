import { createFileRoute } from "@tanstack/react-router";
import { SetupPrototype } from "#/components/setup-prototype/setup";

export const Route = createFileRoute("/prototype/setup")({
	validateSearch: (search: Record<string, unknown>) => ({
		variant:
			search.variant === "B"
				? ("B" as const)
				: search.variant === "C"
					? ("C" as const)
					: ("A" as const),
		scene: typeof search.scene === "string" ? search.scene : "empty",
		theme: search.theme === "dark" ? ("dark" as const) : ("light" as const),
	}),
	component: Page,
});

function Page() {
	const search = Route.useSearch();
	const navigate = Route.useNavigate();
	return (
		<SetupPrototype
			key={search.scene}
			{...search}
			onSearch={(next) =>
				void navigate({ search: (previous) => ({ ...previous, ...next }), replace: true })
			}
		/>
	);
}
