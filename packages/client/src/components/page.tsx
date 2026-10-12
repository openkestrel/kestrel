import { Link } from "@tanstack/react-router";
import type { ReactNode, Ref } from "react";

export function Page({ children }: { children: ReactNode }) {
	return (
		<main className="mx-auto grid min-h-dvh w-full max-w-2xl content-start gap-6 px-6 py-12 sm:py-20">
			<Link to="/" className="w-fit font-medium">
				kestrel
			</Link>
			{children}
		</main>
	);
}

export function PageTitle({
	children,
	ref,
}: {
	children: ReactNode;
	ref?: Ref<HTMLHeadingElement>;
}) {
	return (
		<h1 ref={ref} tabIndex={-1} className="text-xl font-semibold">
			{children}
		</h1>
	);
}
