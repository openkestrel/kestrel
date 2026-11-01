import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Page, PageTitle } from "#/components/page";
import { Refusal, RetryFallback } from "#/components/refusal";
import { ConnectGithub } from "#/components/settings/github";
import { IntegrationCard } from "#/components/settings/integration";
import { ProjectForm } from "#/components/settings/project-form";
import { Button } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import { useDelayed } from "#/lib/use-delayed";
import type { Project } from "#/operator/generated";
import { projectsQuery } from "#/operator/opening";
import {
	A_NEW_PROJECT,
	authorities,
	draftOfProject,
	integrationsQuery,
} from "#/operator/repositories";

type Editing = { declared: Project | undefined };

export function RepositoriesSettings({ organization }: { organization: string }) {
	const projects = useQuery(projectsQuery(organization));
	const integrations = useQuery(integrationsQuery(organization));
	const delayed = useDelayed(projects.isPending);
	const [editing, setEditing] = useState<Editing | null>(null);
	const [connecting, setConnecting] = useState(false);
	const [saved, setSaved] = useState<string>();
	const github = (integrations.data ?? []).filter(({ kind }) => kind === "github");

	// A Project keeps its selection through a pause or a retirement, so the listing says which.
	function standing(id: string): string {
		const state = github.find((integration) => integration.id === id)?.state;
		return state === "disabled"
			? ", which is paused"
			: state === "retired"
				? ", which is retired"
				: "";
	}

	function edit(declared: Project | undefined) {
		setSaved(undefined);
		setEditing({ declared });
	}

	return (
		<Page>
			<PageTitle>Repositories</PageTitle>
			<p className="text-muted-foreground text-sm">
				A Project names the repositories a Workspace checks out and the branch it starts from. A
				GitHub Integration lends a Session the App's authority to push to the one repository it
				watches.
			</p>
			<nav aria-label="Organization" className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
				<Link className="underline" to="/organizations/$organization" params={{ organization }}>
					Workspaces
				</Link>
				<Link
					className="underline"
					to="/organizations/$organization/settings/agents"
					params={{ organization }}
				>
					Agents
				</Link>
				<Link className="underline" to="/setup" search={{ organization }}>
					Setup and readiness
				</Link>
			</nav>

			<section aria-label="Declared Projects" className="grid gap-3">
				<h2 className="font-semibold text-sm">Projects</h2>
				{projects.isPending &&
					(delayed ? (
						<output data-projects-read="delayed" className="text-sm">
							Still reading the Projects; the control plane has not answered yet.
						</output>
					) : (
						<Skeleton className="h-16 w-full" />
					))}
				{projects.isError && (
					<div data-projects-read="failed" className="grid gap-1 text-sm">
						<Refusal error={projects.error} retry={() => void projects.refetch()} />
						<RetryFallback
							error={projects.error}
							retry={() => void projects.refetch()}
							label="Read the Projects again"
							disabled={projects.isFetching}
						/>
						{projects.data && (
							<p className="text-muted-foreground text-xs">Showing the Projects as last read.</p>
						)}
					</div>
				)}
				{projects.data &&
					(projects.data.length === 0 ? (
						<p data-projects-read="empty" className="text-muted-foreground text-sm">
							No Project is declared yet. Declare one with a repository's address, or connect GitHub
							to choose one.
						</p>
					) : (
						<ul className="grid gap-2">
							{projects.data.map((project) => (
								<li
									key={project.id}
									data-project={project.name}
									className="grid gap-1 border p-3 text-sm"
								>
									<div className="flex flex-wrap items-center gap-x-3">
										<span className="font-medium">{project.name}</span>
										<span className="text-muted-foreground text-xs">
											checkout base branch <span className="font-mono">{project.branch}</span>
										</span>
										<Button
											className="ml-auto"
											size="sm"
											type="button"
											variant="outline"
											aria-label={`Edit ${project.name}`}
											onClick={() => edit(project)}
										>
											Edit
										</Button>
									</div>
									<ul className="grid gap-0.5 text-xs">
										{project.repositories.map((repository) => {
											const selected = project.integrations.find(
												(selection) => selection.repository === repository,
											);
											return (
												<li key={repository}>
													<span className="break-all font-mono">{repository}</span>
													<span className="text-muted-foreground">
														{selected
															? ` · reaches GitHub as ${selected.integration}${standing(selected.integration_id)}`
															: " · no App authority"}
													</span>
												</li>
											);
										})}
									</ul>
								</li>
							))}
						</ul>
					))}
			</section>

			{saved && <output className="text-sm">Saved the Project {saved}.</output>}

			{editing ? (
				<ProjectForm
					key={editing.declared?.id ?? "new"}
					organization={organization}
					purpose="declaration"
					heading={editing.declared ? `Edit ${editing.declared.name}` : "Declare a Project"}
					declared={editing.declared !== undefined}
					initial={editing.declared ? draftOfProject(editing.declared) : A_NEW_PROJECT}
					authorities={authorities(integrations.data ?? [])}
					onSaved={(project) => {
						setEditing(null);
						setSaved(project.name);
					}}
					onCancel={() => setEditing(null)}
				/>
			) : (
				<div>
					<Button type="button" onClick={() => edit(undefined)}>
						Declare a Project
					</Button>
				</div>
			)}

			<section aria-label="GitHub Integrations" className="grid gap-3">
				<h2 className="font-semibold text-sm">GitHub</h2>
				{integrations.isPending && <Skeleton className="h-12 w-full" />}
				{integrations.isError && (
					<div data-integrations-read="failed" className="grid gap-1 text-sm">
						<Refusal error={integrations.error} retry={() => void integrations.refetch()} />
						<RetryFallback
							error={integrations.error}
							retry={() => void integrations.refetch()}
							label="Read the Integrations again"
							disabled={integrations.isFetching}
						/>
					</div>
				)}
				{integrations.data &&
					(github.length === 0 ? (
						<p data-integrations-read="empty" className="text-muted-foreground text-sm">
							No GitHub Integration is connected. Without one a Session can read a public repository
							but has nothing to push with.
						</p>
					) : (
						<ul className="grid gap-2">
							{github.map((integration) => (
								<IntegrationCard
									key={integration.id}
									organization={organization}
									integration={integration}
									retry={() => void integrations.refetch()}
								/>
							))}
						</ul>
					))}
				{connecting ? (
					<ConnectGithub organization={organization} />
				) : (
					<div>
						<Button type="button" variant="outline" onClick={() => setConnecting(true)}>
							Connect GitHub
						</Button>
					</div>
				)}
			</section>
		</Page>
	);
}
