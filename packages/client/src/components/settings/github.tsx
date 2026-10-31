import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { described, Field, placement } from "#/components/field";
import { Page, PageTitle } from "#/components/page";
import { Refusal, RetryFallback } from "#/components/refusal";
import { ProjectForm } from "#/components/settings/project-form";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { Skeleton } from "#/components/ui/skeleton";
import { useDelayed } from "#/lib/use-delayed";
import { useNow } from "#/lib/use-now";
import type {
	GithubAppFlow,
	GithubAppRepository,
	Integration,
	Project,
} from "#/operator/generated";
import { projectsQuery } from "#/operator/opening";
import {
	authorities,
	cancelFlow,
	chosenRepository,
	expiresIn,
	finishFlow,
	flowQuery,
	flowRepositoriesQuery,
	flowStage,
	integrationsQuery,
	setWebhookUrl,
	startGithubApp,
	webhookQuery,
} from "#/operator/repositories";

const NEVER_REMOTE =
	"Nothing here deletes or uninstalls the GitHub App: cancelling or expiring erases only kestrel's temporary setup.";

const CONNECT_IDS = { name: "github-name", app_organization: "github-app-organization" };

export function ConnectGithub({
	organization,
	named = "github",
	nested = false,
}: {
	organization: string;
	named?: string;
	nested?: boolean;
}) {
	const [name, setName] = useState(named);
	const [owner, setOwner] = useState("");
	const starting = useMutation({
		mutationFn: () => startGithubApp(organization, name, owner),
		onSuccess: ({ url }) => location.assign(url),
	});
	const { placed, atTheTop, repair, correct } = placement(starting.error, {}, CONNECT_IDS);
	const start = () => {
		if (name.trim() !== "") starting.mutate();
	};

	const Shell = nested ? "fieldset" : "form";
	return (
		<Shell
			aria-label="Connect GitHub"
			className="grid gap-4 border p-4"
			data-connect-github
			onSubmit={(event: FormEvent) => {
				event.preventDefault();
				start();
			}}
		>
			<h2 className="font-semibold text-sm">Connect GitHub</h2>
			<p className="text-muted-foreground text-xs">
				kestrel creates a GitHub App for you on GitHub. Install it, then choose the one repository
				it connects. Nothing is connected until you choose; the setup expires after an hour.
			</p>
			{atTheTop && <Refusal error={starting.error} correct={correct} />}
			<Field
				label="Integration name"
				htmlFor={CONNECT_IDS.name}
				error={placed("name")}
				repair={repair("name")}
				hint="What a Project selects to reach GitHub as."
			>
				<Input
					id={CONNECT_IDS.name}
					value={name}
					autoComplete="off"
					onChange={(event) => setName(event.target.value)}
					aria-invalid={placed("name") !== undefined}
					aria-describedby={described(CONNECT_IDS.name, true, placed("name") !== undefined)}
				/>
			</Field>
			<Field
				label="GitHub organization"
				htmlFor={CONNECT_IDS.app_organization}
				error={placed("app_organization")}
				repair={repair("app_organization")}
				hint="Optional: create the App under this organization instead of your own account."
			>
				<Input
					id={CONNECT_IDS.app_organization}
					value={owner}
					autoComplete="off"
					spellCheck={false}
					onChange={(event) => setOwner(event.target.value)}
					aria-invalid={placed("app_organization") !== undefined}
					aria-describedby={described(
						CONNECT_IDS.app_organization,
						true,
						placed("app_organization") !== undefined,
					)}
				/>
			</Field>
			<div>
				<Button
					type={nested ? "button" : "submit"}
					disabled={starting.isPending || starting.isSuccess || !name.trim()}
					onClick={nested ? start : undefined}
				>
					{starting.isPending || starting.isSuccess
						? "Opening GitHub…"
						: "Create the App on GitHub"}
				</Button>
			</div>
		</Shell>
	);
}

export function GithubSetup({ organization, flow }: { organization?: string; flow?: string }) {
	return (
		<Page>
			<PageTitle>Connect GitHub</PageTitle>
			{flow === undefined ? (
				<>
					<p className="text-sm" data-no-flow>
						There is no GitHub setup to resume here.
					</p>
					<Onward organization={organization} />
				</>
			) : (
				<Flow organization={organization} flow={flow} />
			)}
		</Page>
	);
}

function Flow({ organization, flow }: { organization?: string; flow: string }) {
	const queryClient = useQueryClient();
	const read = useQuery(flowQuery(flow));
	const delayed = useDelayed(read.isPending);
	const now = useNow(30_000);
	const [chosen, setChosen] = useState<GithubAppRepository>();
	const scope = read.data?.organization ?? organization;

	const settle = (settled: GithubAppFlow) =>
		queryClient.setQueryData(flowQuery(flow).queryKey, settled);
	const finishing = useMutation({
		mutationFn: (repository: GithubAppRepository) => finishFlow(flow, repository.repository),
		onSuccess: async (settled, repository) => {
			setChosen(repository);
			settle(settled);
			await queryClient.invalidateQueries({ queryKey: ["organizations", settled.organization] });
			await queryClient.invalidateQueries({ queryKey: ["readiness"] });
		},
	});
	const cancelling = useMutation({ mutationFn: () => cancelFlow(flow), onSuccess: settle });

	const shown = read.data;
	const stage = shown && flowStage(shown);

	return (
		<>
			{read.isPending &&
				(delayed ? (
					<output data-flow-read="delayed" className="text-sm">
						Still reading the GitHub setup; the control plane has not answered yet.
					</output>
				) : (
					<Skeleton className="h-16 w-full" />
				))}
			{read.isError && (
				<div data-flow-read="failed" className="grid gap-1 text-sm">
					<Refusal error={read.error} retry={() => void read.refetch()} />
					<RetryFallback
						error={read.error}
						retry={() => void read.refetch()}
						label="Read the GitHub setup again"
						disabled={read.isFetching}
					/>
					{shown && (
						<p className="text-muted-foreground text-xs">Showing the setup as last read.</p>
					)}
				</div>
			)}
			{shown && (
				<p className="text-muted-foreground text-xs" data-flow-expiry>
					{stage === "connected"
						? `Connected the repository ${shown.repository ?? ""} as the Integration ${shown.integration ?? ""}.`
						: stage === "ended"
							? `${shown.phase === "expired" ? "Expired. " : ""}${NEVER_REMOTE}`
							: `${expiresIn(shown.expires_at, now)}, at ${new Date(shown.expires_at).toLocaleTimeString()}. ${NEVER_REMOTE}`}
				</p>
			)}
			{shown && stage === "installing" && (
				<section
					aria-label="Installing the App"
					className="grid gap-2 text-sm"
					data-flow-stage="installing"
				>
					<p>Install the App on GitHub; a repository can be chosen only once it is installed.</p>
					<p className="text-muted-foreground text-xs">{shown.guidance}</p>
					<Button
						type="button"
						variant="outline"
						className="justify-self-start"
						disabled={read.isFetching}
						onClick={() => void read.refetch()}
					>
						Check the installation again
					</Button>
				</section>
			)}
			{shown && stage === "choosing" && (
				<>
					<RepositoryPicker
						flow={flow}
						finishing={finishing.isPending}
						onChoose={(repository) => finishing.mutate(repository)}
					/>
					{finishing.isError && (
						<Refusal error={finishing.error} retry={() => void read.refetch()} />
					)}
				</>
			)}
			{shown && stage === "connected" && <Connected flow={shown} chosen={chosen} />}
			{shown && stage === "ended" && (
				<section aria-label="Setup ended" className="grid gap-2 text-sm" data-flow-stage="ended">
					<p>
						{shown.phase === "cancelled"
							? "This GitHub setup was cancelled."
							: shown.phase === "expired"
								? "This GitHub setup expired before a repository was chosen."
								: "GitHub's one-time App conversion failed or could not be confirmed, so this setup cannot continue."}
					</p>
					<p className="text-muted-foreground text-xs">{shown.guidance}</p>
					{scope && <ConnectGithub organization={scope} named={shown.name} />}
				</section>
			)}
			{shown && (stage === "installing" || stage === "choosing") && (
				<div className="grid gap-1">
					<Button
						type="button"
						variant="outline"
						className="justify-self-start"
						disabled={cancelling.isPending || finishing.isPending}
						onClick={() => cancelling.mutate()}
					>
						Cancel this GitHub setup
					</Button>
					{cancelling.isError && (
						<Refusal error={cancelling.error} retry={() => void read.refetch()} />
					)}
				</div>
			)}
			<Onward organization={scope} />
		</>
	);
}

function RepositoryPicker({
	flow,
	finishing,
	onChoose,
}: {
	flow: string;
	finishing: boolean;
	onChoose: (repository: GithubAppRepository) => void;
}) {
	const [page, setPage] = useState(1);
	const [selected, setSelected] = useState<GithubAppRepository>();
	const listed = useQuery({
		...flowRepositoriesQuery(flow, page),
		placeholderData: (held) => held,
	});
	const delayed = useDelayed(listed.isPending);

	return (
		<fieldset className="grid gap-3" data-repository-picker>
			<legend className="mb-2 font-semibold text-sm">Choose the repository to connect</legend>
			<p className="text-muted-foreground text-xs">
				These are the repositories the App's installation reaches now. The connection watches
				exactly one.
			</p>
			{listed.isPending &&
				(delayed ? (
					<output data-repositories-read="delayed" className="text-sm">
						Still asking GitHub for the repositories.
					</output>
				) : (
					<Skeleton className="h-16 w-full" />
				))}
			{listed.isError && (
				<div data-repositories-read="failed" className="grid gap-1 text-sm">
					<Refusal error={listed.error} retry={() => void listed.refetch()} />
					<RetryFallback
						error={listed.error}
						retry={() => void listed.refetch()}
						label="Ask GitHub again"
						disabled={listed.isFetching}
					/>
				</div>
			)}
			{listed.data &&
				!listed.isError &&
				(listed.data.total === 0 ? (
					<div data-repositories-read="empty" className="grid gap-1 text-sm">
						<p>
							The App's installation reaches no repository yet. Give it one on GitHub, then ask
							again.
						</p>
						<Button
							type="button"
							size="xs"
							variant="outline"
							className="justify-self-start"
							onClick={() => void listed.refetch()}
						>
							Ask GitHub again
						</Button>
					</div>
				) : (
					<>
						<ul className="grid gap-1">
							{listed.data.repositories.map((repository) => (
								<li key={repository.id}>
									<label
										className="flex flex-wrap items-center gap-x-3 gap-y-1 border p-3 text-sm"
										data-repository={repository.repository}
									>
										<input
											type="radio"
											name="github-repository"
											value={repository.repository}
											checked={selected?.id === repository.id}
											onChange={() => setSelected(repository)}
										/>
										<span className="font-medium">{repository.repository}</span>
										<span className="text-muted-foreground text-xs">
											default branch <span className="font-mono">{repository.default_branch}</span>
											{repository.private ? " · private" : " · public"}
										</span>
									</label>
								</li>
							))}
						</ul>
						<nav aria-label="Repository pages" className="flex items-center gap-2 text-xs">
							<Button
								type="button"
								size="xs"
								variant="outline"
								disabled={page <= 1 || listed.isFetching}
								onClick={() => setPage((at) => at - 1)}
							>
								Previous page
							</Button>
							<output data-repositories-page>
								Page {listed.data.page} · {listed.data.total} repositories
							</output>
							<Button
								type="button"
								size="xs"
								variant="outline"
								disabled={listed.data.next_page === null || listed.isFetching}
								onClick={() => setPage(listed.data.next_page ?? page)}
							>
								Next page
							</Button>
						</nav>
					</>
				))}
			<div>
				<Button
					type="button"
					disabled={selected === undefined || finishing}
					onClick={() => selected && onChoose(selected)}
				>
					{finishing
						? "Connecting…"
						: selected
							? `Connect ${selected.repository}`
							: "Choose a repository"}
				</Button>
			</div>
		</fieldset>
	);
}

function Connected({ flow, chosen }: { flow: GithubAppFlow; chosen?: GithubAppRepository }) {
	const projects = useQuery(projectsQuery(flow.organization));
	const integrations = useQuery(integrationsQuery(flow.organization));
	const [saved, setSaved] = useState<Project>();
	const selecting = projects.data?.find((project) =>
		project.integrations.some(({ integration }) => integration === flow.integration),
	);

	if (saved || selecting)
		return (
			<output className="text-sm" data-flow-project>
				{saved ? "Saved" : "Already saved"} the Project {(saved ?? selecting)?.name}, which reaches
				GitHub as {flow.integration}.
			</output>
		);

	return (
		<>
			{integrations.isPending && <Skeleton className="h-16 w-full" />}
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
			{projects.isError && (
				<div data-projects-read="failed" className="grid gap-1 text-sm">
					<Refusal error={projects.error} retry={() => void projects.refetch()} />
					<RetryFallback
						error={projects.error}
						retry={() => void projects.refetch()}
						label="Read the Projects again"
					/>
				</div>
			)}
			{/* Its Integration is chosen for it, so the form waits until that choice can be shown. */}
			{integrations.data && (
				<ProjectForm
					organization={flow.organization}
					purpose="declaration"
					heading="Declare its Project"
					initial={chosenRepository(flow, chosen)}
					authorities={authorities(integrations.data)}
					onSaved={setSaved}
				/>
			)}
		</>
	);
}

export function Webhook({
	organization,
	integration,
}: {
	organization: string;
	integration: Integration;
}) {
	const queryClient = useQueryClient();
	const read = useQuery(webhookQuery(organization, integration.name));
	const [base, setBase] = useState("");
	const id = `webhook-base-${integration.id}`;
	const setting = useMutation({
		mutationFn: () => setWebhookUrl(organization, integration, base),
		onSuccess: async () => {
			await queryClient.invalidateQueries({
				queryKey: ["organizations", organization, "integrations"],
			});
		},
	});
	return (
		<div className="grid gap-1 text-xs" data-webhook={integration.name}>
			{read.isPending && <Skeleton className="h-4 w-48" />}
			{read.isError && (
				<div data-webhook-read="failed" className="grid gap-1">
					<Refusal error={read.error} retry={() => void read.refetch()} />
					<RetryFallback
						error={read.error}
						retry={() => void read.refetch()}
						label="Read the hook again"
						disabled={read.isFetching}
					/>
				</div>
			)}
			{read.data && (
				<>
					<p>
						Hook <code className="break-all font-mono">{read.data.url}</code>
						{read.data.pending_url && (
							<span className="block" data-webhook-pending>
								An update to <code className="break-all font-mono">{read.data.pending_url}</code> is
								unfinished; set the URL again to finish it.
							</span>
						)}
					</p>
					<p className="text-muted-foreground" data-webhook-deliveries>
						{read.data.deliveries.length === 0
							? "GitHub records no recent Delivery, which says nothing either way about reachability. kestrel polls the Delivery log regardless."
							: `Latest Delivery results: ${read.data.deliveries
									.slice(0, 5)
									.map(({ status_code }) => status_code ?? "unrecorded")
									.join(", ")}.`}
					</p>
				</>
			)}
			<form
				aria-label={`Set ${integration.name}'s hook URL`}
				className="flex flex-wrap items-end gap-2"
				onSubmit={(event) => {
					event.preventDefault();
					setting.mutate();
				}}
			>
				<div className="grid gap-1">
					<Label htmlFor={id}>Public webhook base</Label>
					<Input
						id={id}
						value={base}
						placeholder="The deployment's own"
						autoComplete="off"
						spellCheck={false}
						onChange={(event) => setBase(event.target.value)}
					/>
				</div>
				<Button type="submit" size="xs" variant="outline" disabled={setting.isPending}>
					{setting.isPending ? "Setting…" : "Set the hook URL"}
				</Button>
			</form>
			{setting.isSuccess && (
				<output>
					GitHub took the new hook URL; that says nothing yet about a Delivery reaching kestrel.
				</output>
			)}
			{setting.isError && <Refusal error={setting.error} retry={() => void read.refetch()} />}
		</div>
	);
}

function Onward({ organization }: { organization?: string }) {
	return (
		<nav aria-label="Setup" className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
			<Link className="underline" to="/setup" search={organization ? { organization } : {}}>
				Setup and readiness
			</Link>
			{organization && (
				<Link
					className="underline"
					to="/organizations/$organization/settings/repositories"
					params={{ organization }}
				>
					Repositories
				</Link>
			)}
		</nav>
	);
}
