import { queryOptions } from "@tanstack/react-query";
import type {
	GithubAppFlow,
	GithubAppRepositories,
	GithubAppRepository,
	GithubAppStarted,
	Integration,
	IntegrationWebhook,
	Project,
	ProjectDeclaration,
	RepositoryPurpose,
	RepositoryResolution,
	RepositoryResolutionRequest,
} from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export const integrationsQuery = (organization: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "integrations"],
		queryFn: ({ signal }) =>
			operator.read<Integration[]>(operatorPath("organizations", organization, "integrations"), {
				signal,
			}),
	});

export const webhookQuery = (organization: string, integration: string) =>
	queryOptions({
		queryKey: ["organizations", organization, "integrations", integration, "webhook"],
		queryFn: ({ signal }) =>
			operator.read<IntegrationWebhook>(
				operatorPath("organizations", organization, "integrations", integration, "webhook"),
				{ signal },
			),
	});

export const flowQuery = (flow: string) =>
	queryOptions({
		queryKey: ["github-app", flow],
		queryFn: ({ signal }) =>
			operator.read<GithubAppFlow>(operatorPath("github-app", "flows", flow), { signal }),
	});

export const PER_PAGE = 30;

export const flowRepositoriesQuery = (flow: string, page: number) =>
	queryOptions({
		queryKey: ["github-app", flow, "repositories", page],
		queryFn: ({ signal }) =>
			operator.read<GithubAppRepositories>(
				`${operatorPath("github-app", "flows", flow, "repositories")}?${new URLSearchParams({
					page: String(page),
					per_page: String(PER_PAGE),
				})}`,
				{ signal },
			),
	});

// The operator pages GitHub returns to sit behind the Client's own origin, which is loopback in every supported install.
export function startGithubApp(
	organization: string,
	name: string,
	appOrganization: string,
): Promise<GithubAppStarted> {
	return operator.write<GithubAppStarted>(
		"POST",
		operatorPath("organizations", organization, "github-app"),
		{
			name: name.trim(),
			callback_base: location.origin,
			...(appOrganization.trim() !== "" && { app_organization: appOrganization.trim() }),
		},
	);
}

export function finishFlow(flow: string, repository: string): Promise<GithubAppFlow> {
	return operator.write<GithubAppFlow>(
		"POST",
		operatorPath("github-app", "flows", flow, "finish"),
		{ repository },
	);
}

export function cancelFlow(flow: string): Promise<GithubAppFlow> {
	return operator.write<GithubAppFlow>("DELETE", operatorPath("github-app", "flows", flow));
}

export function setWebhookUrl(
	organization: string,
	integration: Integration,
	base: string,
): Promise<Integration> {
	return operator.write<Integration>(
		"PUT",
		operatorPath("organizations", organization, "integrations", integration.name, "webhook"),
		{ ...(base.trim() !== "" && { base: base.trim() }), revision: integration.revision },
	);
}

export function resolveRepositories(
	request: RepositoryResolutionRequest,
): Promise<RepositoryResolution> {
	return operator.write<RepositoryResolution>(
		"POST",
		operatorPath("repository-resolutions"),
		request,
	);
}

export function declareProject(
	organization: string,
	declaration: ProjectDeclaration,
): Promise<Project> {
	return operator.write<Project>(
		"POST",
		operatorPath("organizations", organization, "projects"),
		declaration,
	);
}

export type RepositoryRow = { address: string; integration: string };

export type ProjectDraft = {
	name: string;
	repositories: RepositoryRow[];
	branch: string;
};

export const A_NEW_PROJECT: ProjectDraft = {
	name: "",
	repositories: [{ address: "", integration: "" }],
	branch: "",
};

function given(draft: ProjectDraft): RepositoryRow[] {
	return draft.repositories
		.map(({ address, integration }) => ({ address: address.trim(), integration }))
		.filter(({ address }) => address !== "");
}

function selections(rows: RepositoryRow[]) {
	return rows
		.filter(({ integration }) => integration !== "")
		.map(({ address, integration }) => ({ repository: address, integration }));
}

export function projectDeclaration(draft: ProjectDraft): ProjectDeclaration {
	const rows = given(draft);
	return {
		name: draft.name.trim(),
		repositories: rows.map(({ address }) => address),
		integrations: selections(rows),
		branch: draft.branch.trim(),
	};
}

export function resolutionRequest(
	draft: ProjectDraft,
	organization: string,
	purpose: RepositoryPurpose,
): RepositoryResolutionRequest {
	const rows = given(draft);
	const repositories = rows.map(({ address }) => address);
	if (purpose === "public_setup") return { repositories, purpose };
	return { repositories, purpose, organization, integrations: selections(rows) };
}

export function draftOfProject(project: Project): ProjectDraft {
	return {
		name: project.name,
		branch: project.branch,
		repositories: project.repositories.map((address) => ({
			address,
			integration:
				project.integrations.find((selected) => selected.repository === address)?.integration ?? "",
		})),
	};
}

export function chosenRepository(
	flow: GithubAppFlow,
	repository: GithubAppRepository,
): ProjectDraft {
	return {
		name: repository.repository.split("/").at(-1) ?? repository.repository,
		branch: repository.default_branch,
		repositories: [{ address: repository.repository, integration: flow.integration ?? "" }],
	};
}

export type FlowStage = "installing" | "choosing" | "connected" | "ended";

const ENDED = new Set<GithubAppFlow["phase"]>(["cancelled", "expired", "conversion_failed"]);

export function flowStage(flow: GithubAppFlow): FlowStage {
	if (ENDED.has(flow.phase)) return "ended";
	if (flow.phase === "select_repository") return "choosing";
	if (flow.phase === "completed" && flow.integration !== null) return "connected";
	return "installing";
}

export function expiresIn(expiresAt: string, now: number): string {
	const left = Date.parse(expiresAt) - now;
	if (left <= 0) return "Expired";
	const minutes = Math.ceil(left / 60_000);
	return `Expires in ${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
}

// Only these lend a repository the App's authority; a declaration naming another is refused.
export function authorities(integrations: Integration[]): Integration[] {
	return integrations.filter(
		(integration) =>
			integration.kind === "github" &&
			integration.state === "enabled" &&
			integration.carries.includes("outbound"),
	);
}
