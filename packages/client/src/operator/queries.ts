import { type QueryClient, queryOptions } from "@tanstack/react-query";
import type { Change, Organization, Queue, Session, Workspace, WorkspaceWork } from "./generated";
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
			operator.read<Workspace[]>(operatorPath("organizations", organization, "workspaces"), {
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

// Notices name a Workspace by id but read keys carry its name, so the cache maps between them.
export function refetchNoticed(
	client: QueryClient,
	organization: string,
	notice: Change,
): Promise<void> {
	if (notice.resource === "workspace") {
		const named = workspaceNames(client, organization, notice.id);
		return client.invalidateQueries({
			queryKey: workspacesQuery(organization).queryKey,
			predicate: ({ queryKey }) => queryKey.length === 3 || named.has(queryKey[3]),
		});
	}
	if (notice.resource === "session") {
		const holding = sessionHolders(client, organization, notice.id);
		return client.invalidateQueries({
			queryKey: sessionsKey(organization),
			predicate: ({ queryKey }) =>
				queryKey[3] === notice.id ||
				(queryKey[3] === "workspace" && (holding.size === 0 || holding.has(queryKey[4]))),
		});
	}
	return client.invalidateQueries({ queryKey: queueKey(organization) });
}

function workspaceNames(client: QueryClient, organization: string, id: string): Set<unknown> {
	const known = [...(client.getQueryData(workspacesQuery(organization).queryKey) ?? [])];
	for (const query of client
		.getQueryCache()
		.findAll({ queryKey: workspacesQuery(organization).queryKey })) {
		const name = query.queryKey[3];
		if (query.queryKey.length !== 4 || typeof name !== "string") continue;
		const read = client.getQueryData(workspaceQuery(organization, name).queryKey);
		if (read) known.push(read);
	}
	return new Set(
		known.filter((workspace) => workspace.id === id).map((workspace) => workspace.name),
	);
}

// Empty for a Session new to this tab, which then refetches every Workspace's Session list.
function sessionHolders(client: QueryClient, organization: string, id: string): Set<unknown> {
	const holding = new Set<unknown>();
	for (const query of client
		.getQueryCache()
		.findAll({ queryKey: [...sessionsKey(organization), "workspace"] })) {
		const name = query.queryKey[4];
		if (typeof name !== "string") continue;
		const sessions = client.getQueryData(workspaceSessionsQuery(organization, name).queryKey);
		if (sessions?.some((session) => session.id === id)) holding.add(name);
	}
	return holding;
}
