import { queryOptions } from "@tanstack/react-query";
import type { Organization, Workspace } from "./generated";
import { operatorPath, transport } from "./transport";

export const operator = transport();

export const organizationsQuery = queryOptions({
	queryKey: ["organizations"],
	queryFn: ({ signal }) => operator.read<Organization[]>(operatorPath("organizations"), { signal }),
});

export const workspacesQuery = (organization: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "workspaces"],
		queryFn: ({ signal }) =>
			operator.read<Workspace[]>(operatorPath("organizations", organization, "workspaces"), {
				signal,
			}),
	});

export const workspaceQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "workspaces", workspace],
		queryFn: ({ signal }) =>
			operator.read<Workspace>(
				operatorPath("organizations", organization, "workspaces", workspace),
				{ signal },
			),
	});
