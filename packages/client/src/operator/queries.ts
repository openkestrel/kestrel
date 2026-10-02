import { type QueryClient, queryOptions } from "@tanstack/react-query";
import type {
	Change,
	Organization,
	Queue,
	Session,
	Workspace,
	WorkspaceListed,
	WorkspaceWork,
} from "./generated";
import { type Delivered, readRange, type Range } from "./transcript";
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
			operator.read<WorkspaceListed[]>(operatorPath("organizations", organization, "workspaces"), {
				signal,
			}),
	});

export const workspaceKey = (organization: string, workspace: string) =>
	["organizations", organization, "workspaces", workspace] as const;

export const workspaceQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: workspaceKey(organization, workspace),
		queryFn: ({ signal }) =>
			operator.read<Workspace>(
				operatorPath("organizations", organization, "workspaces", workspace),
				{ signal },
			),
	});

export const sessionsKey = (organization: string) => ["organizations", organization, "sessions"];

export const sessionQuery = (organization: string, session: string) =>
	queryOptions({
		queryKey: [...sessionsKey(organization), session],
		queryFn: ({ signal }) =>
			operator.read<Session>(operatorPath("organizations", organization, "sessions", session), {
				signal,
			}),
	});

// Oldest first, so the Session a row shows is the last.
export const workspaceSessionsQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: [...sessionsKey(organization), "workspace", workspace],
		queryFn: ({ signal }) =>
			operator.read<Session[]>(
				operatorPath("organizations", organization, "workspaces", workspace, "sessions"),
				{ signal },
			),
	});

export const workQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: [...workspaceKey(organization, workspace), "work"],
		queryFn: ({ signal }) =>
			operator.read<WorkspaceWork>(
				operatorPath("organizations", organization, "workspaces", workspace, "work"),
				{ signal },
			),
	});

export const queueKey = (organization: string) => ["organizations", organization, "queue"];

export const queueQuery = (organization: string) =>
	queryOptions({
		queryKey: queueKey(organization),
		queryFn: ({ signal }) =>
			operator.read<Queue>(operatorPath("organizations", organization, "queue"), { signal }),
	});

// Outside the Organization/Workspace prefix so a change notice never refetches recorded entries.
export const transcriptKey = (organization: string, workspace: string) =>
	["transcript", organization, workspace] as const;

export const transcriptRangeQuery = (organization: string, workspace: string, range: Range) =>
	queryOptions({
		queryKey: [...transcriptKey(organization, workspace), range.first, range.last],
		queryFn: ({ signal }): Promise<Delivered[]> =>
			readRange(operator, organization, workspace, range, signal),
	});

export const transcriptPayloadQuery = (organization: string, workspace: string, payload: string) =>
	queryOptions({
		queryKey: [...transcriptKey(organization, workspace), "payload", payload],
		queryFn: ({ signal }) =>
			operator.readText(
				operatorPath(
					"organizations",
					organization,
					"workspaces",
					workspace,
					"transcript",
					"payloads",
					payload,
				),
				{ signal },
			),
	});

export function refetchOrganization(client: QueryClient, organization: string): Promise<void> {
	return client.invalidateQueries({
		queryKey: organizationsQuery.queryKey,
		predicate: ({ queryKey }) => queryKey.length === 1 || queryKey[1] === organization,
	});
}

// The Workspace list carries each latest Session and its queue standing, so every notice refetches it.
export async function refetchNoticed(
	client: QueryClient,
	organization: string,
	change: Change,
): Promise<void> {
	const list = client.invalidateQueries({
		queryKey: workspacesQuery(organization).queryKey,
		exact: true,
	});
	switch (change.resource) {
		case "workspace":
			await Promise.all([
				list,
				client.invalidateQueries({ queryKey: workspaceKey(organization, change.workspace) }),
			]);
			return;
		case "session":
			await Promise.all([
				list,
				client.invalidateQueries({ queryKey: [...sessionsKey(organization), change.id] }),
				client.invalidateQueries({
					queryKey: workspaceSessionsQuery(organization, change.workspace).queryKey,
				}),
			]);
			return;
		case "queue":
			await Promise.all([list, client.invalidateQueries({ queryKey: queueKey(organization) })]);
	}
}
