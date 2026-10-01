import { queryOptions } from "@tanstack/react-query";
import type { Organization, Queue, Session, Workspace, WorkspaceWork } from "./generated";
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

// A Workspace's Sessions, oldest first; its latest Session is the one a row shows.
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

// Expansions and payloads are lazy reads of recorded entries, not current-state views: they sit
// outside the Organization/Workspace prefix so a change notice never refetches them.
export const transcriptKey = (organization: string, workspace: string) =>
	["transcript", organization, workspace] as const;

// One Activity's expansion: its own seq range, every kind, read on demand.
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
// The read a notice names. Every view of a changed resource matches one of these prefixes.
export function noticedKey(
	organization: string,
	resource: "workspace" | "session" | "queue",
): readonly unknown[] {
	const keys: Record<typeof resource, readonly unknown[]> = {
		workspace: ["organizations", organization, "workspaces"],
		session: sessionsKey(organization),
		queue: queueKey(organization),
	};
	return keys[resource];
}
