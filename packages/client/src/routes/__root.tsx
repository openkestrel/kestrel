import type { QueryClient } from "@tanstack/react-query";
import { createRootRouteWithContext, HeadContent, Link, Scripts } from "@tanstack/react-router";
import type { ReactNode } from "react";
import type { Currency } from "#/operator/currency";
import appCss from "../styles.css?url";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient; currency: Currency }>()(
	{
		head: () => ({
			meta: [
				{ charSet: "utf-8" },
				{ name: "viewport", content: "width=device-width, initial-scale=1" },
				{ title: "kestrel" },
			],
			links: [{ rel: "stylesheet", href: appCss }],
		}),
		shellComponent: Document,
		notFoundComponent: NotFound,
	},
);

function Document({ children }: { children: ReactNode }) {
	return (
		<html lang="en">
			<head>
				<HeadContent />
			</head>
			<body>
				{children}
				<Scripts />
			</body>
		</html>
	);
}

function NotFound() {
	return (
		<main className="mx-auto max-w-xl p-8">
			<h1 className="font-semibold text-lg">Nothing is here</h1>
			<p className="mt-2 text-muted-foreground text-sm">
				This address names no page in kestrel.{" "}
				<Link to="/" className="underline">
					Start over
				</Link>
				.
			</p>
		</main>
	);
}
