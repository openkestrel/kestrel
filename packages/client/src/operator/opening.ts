import { queryOptions } from "@tanstack/react-query";
import type {
	Agent,
	Opened,
	Project,
	SubscriptionProfileListed,
	WorkspaceDeclaration,
} from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export const projectsQuery = (organization: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "projects"],
		queryFn: ({ signal }) =>
			operator.read<Project[]>(operatorPath("organizations", organization, "projects"), { signal }),
	});

export const agentsQuery = (organization: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "agents"],
		queryFn: ({ signal }) =>
			operator.read<Agent[]>(operatorPath("organizations", organization, "agents"), { signal }),
	});

export const profilesQuery = (organization: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "profiles"],
		queryFn: ({ signal }) =>
			operator.read<SubscriptionProfileListed[]>(
				operatorPath("organizations", organization, "profiles"),
				{ signal },
			),
	});

export async function openWorkspace(
	organization: string,
	declaration: WorkspaceDeclaration,
): Promise<Opened> {
	return operator.write<Opened>(
		"POST",
		operatorPath("organizations", organization, "workspaces"),
		declaration,
	);
}

export function resolvedRepositories(project: Project | undefined): string {
	return project?.repositories.join(", ") ?? "";
}

export function resolvedBranch(branch: string, project: Project | undefined): string {
	return branch.trim() !== "" ? branch : (project?.branch ?? "");
}

export function resolvedModel(model: string, agent: Agent | undefined): string {
	if (model.trim() !== "") {
		return model;
	}
	return agent?.model && agent.model !== "" ? agent.model : "the harness's default";
}

export function resolvedEnvironment(workRole: { driver: string } | null | undefined): string {
	return workRole?.driver ?? "No dispatch configuration is recorded.";
}
