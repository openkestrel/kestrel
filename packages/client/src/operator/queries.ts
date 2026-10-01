import { queryOptions } from "@tanstack/react-query";
import type { Organization, Queue, Session, Workspace } from "./generated";
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

export const queueKey = (organization: string) => ["organizations", organization, "queue"];

export const queueQuery = (organization: string) =>
	queryOptions({
		queryKey: queueKey(organization),
		queryFn: ({ signal }) =>
			operator.read<Queue>(operatorPath("organizations", organization, "queue"), { signal }),
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
