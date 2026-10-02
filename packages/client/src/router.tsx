import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRouter } from "@tanstack/react-router";
import { Currency } from "#/operator/currency";
import { participant } from "#/operator/participant";
import { operator } from "#/operator/queries";
import { Refused } from "#/operator/transport";
import { routeTree } from "./routeTree.gen";

export function getRouter() {
	const queryClient = new QueryClient({
		defaultOptions: {
			queries: {
				retry: (failures, error) =>
					!(error instanceof Refused && error.status < 500) && failures < 2,
			},
		},
	});
	const currency = new Currency({ queryClient, operations: operator, participant });

	return createRouter({
		routeTree,
		context: { queryClient, currency },
		scrollRestoration: true,
		defaultPreload: "intent",
		Wrap: ({ children }) => (
			<QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
		),
	});
}

declare module "@tanstack/react-router" {
	interface Register {
		router: ReturnType<typeof getRouter>;
	}
}
