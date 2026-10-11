import { queryOptions } from "@tanstack/react-query";
import type { Install, Readiness } from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export const installQuery = queryOptions({
	queryKey: ["install"],
	queryFn: ({ signal }) => operator.read<Install>(operatorPath("install"), { signal }),
});

export const readinessQuery = (organization?: string) =>
	queryOptions({
		queryKey: ["readiness", organization ?? null],
		queryFn: ({ signal }) =>
			operator.read<Readiness>(
				operatorPath("readiness") +
					(organization ? `?${new URLSearchParams({ organization })}` : ""),
				{ signal },
			),
	});
