import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRouter } from "@tanstack/react-router";
import { Refused } from "#/operator/transport";
import { routeTree } from "./routeTree.gen";

export function getRouter() {
	const queryClient = new QueryClient({
		defaultOptions: {
			queries: {
				retry: (failures, error) =>
					!(error instanceof Refused && error.status < 500) && failures < 2,
				retryDelay: (failures, error) =>
					error instanceof Refused && error.retryAfter !== undefined
						? error.retryAfter * 1000
						: Math.min(1000 * 2 ** failures, 30_000),
			},
		},
	});
	return createRouter({
		routeTree,
		context: { queryClient },
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
