import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { Page, PageTitle } from "#/components/page";
import { Diagnosis, Refusal, RetryFallback } from "#/components/refusal";
import { ConnectGithub } from "#/components/settings/github";
import { ProjectForm } from "#/components/settings/project-form";
import { Button } from "#/components/ui/button";
import { Label } from "#/components/ui/label";
import {
	Questionnaire,
	QuestionnaireActions,
	QuestionnaireDescription,
	QuestionnaireInput,
	QuestionnaireItem,
	QuestionnaireProgress,
	QuestionnaireTitle,
} from "#/components/ui/questionnaire";
import { Skeleton } from "#/components/ui/skeleton";
import type { Operator, Readiness } from "#/operator/generated";
import { operator } from "#/operator/queries";
import { A_NEW_PROJECT } from "#/operator/repositories";
import { installQuery, readinessQuery } from "#/operator/setup";
import { diagnosisOf, operatorPath } from "#/operator/transport";

export function Setup({ organization }: { organization?: string }) {
	const install = useQuery(installQuery);
	const readiness = useQuery(readinessQuery(organization));
	const queryClient = useQueryClient();
	const [editing, setEditing] = useState(false);
	const [chose, setChose] = useState<"repository" | "remaining">();
	const heading = useRef<HTMLHeadingElement>(null);
	const loaded = install.isSuccess && readiness.isSuccess;
	const naming = readiness.data?.saved.operator === null || editing;
	const step = naming
		? "name"
		: (chose ?? (readiness.data?.saved.projects.length === 0 ? "repository" : "remaining"));

	useEffect(() => {
		if (loaded && !naming) heading.current?.focus();
	}, [naming, loaded]);

	async function refresh() {
		await queryClient.invalidateQueries();
	}

	const error = install.isError ? install.error : readiness.isError ? readiness.error : null;
	return (
		<Page>
			{error !== null ? (
				<>
					<PageTitle>Setup unavailable</PageTitle>
					<Refusal error={error} retry={() => void refresh()} />
					<RetryFallback error={error} retry={() => void refresh()} label="Read again" />
				</>
			) : !install.isSuccess || !readiness.isSuccess ? (
				<Skeleton className="h-24 w-full" />
			) : install.data.initial_setup_complete ? (
				<>
					<PageTitle ref={heading}>Current readiness</PageTitle>
					<p className="text-sm text-muted-foreground">
						Initial setup is complete. Repair any current gaps below.
					</p>
					<Prerequisites readiness={readiness.data} refresh={refresh} />
					<OrganizationLink organization={readiness.data.organization} />
				</>
			) : (
				<>
					<PageTitle ref={heading}>{naming ? "Set up kestrel" : "Continue setup"}</PageTitle>
					<Questionnaire
						item={step}
						items={[{ name: "name" }, { name: "repository" }, { name: "remaining" }]}
						onSubmit={(event) => event.preventDefault()}
					>
						<QuestionnaireProgress />
						<QuestionnaireItem name="name">
							<QuestionnaireTitle>What should kestrel call you?</QuestionnaireTitle>
							<QuestionnaireDescription>
								This name appears when you take a turn in a Workspace.
							</QuestionnaireDescription>
							<NameOperator
								key={readiness.data.saved.operator?.name ?? "unnamed"}
								active={naming}
								saved={readiness.data.saved.operator}
								onSaved={async () => {
									setEditing(false);
									setChose(undefined);
									await refresh();
								}}
							/>
						</QuestionnaireItem>
						<QuestionnaireItem name="repository">
							<QuestionnaireTitle>Which repository should the agent work on?</QuestionnaireTitle>
							<QuestionnaireDescription>
								Connect GitHub to choose one the agent can push to, or give a public repository it
								can only read.
							</QuestionnaireDescription>
							{readiness.data.organization !== null && step === "repository" && (
								<RepositoryStep
									organization={readiness.data.organization}
									projects={readiness.data.saved.projects}
									refresh={refresh}
								/>
							)}
							<QuestionnaireActions>
								<Button
									type="button"
									variant="outline"
									className="min-h-11 justify-self-start sm:min-h-8"
									onClick={() => setEditing(true)}
								>
									Back
								</Button>
								<Button
									type="button"
									className="col-start-3 min-h-11 sm:min-h-8"
									onClick={() => setChose("remaining")}
								>
									Continue
								</Button>
							</QuestionnaireActions>
						</QuestionnaireItem>
						<QuestionnaireItem name="remaining">
							<QuestionnaireTitle>Remaining prerequisites</QuestionnaireTitle>
							<QuestionnaireDescription>
								Your saved steps are reusable. Setup finishes with the first confirmed Session
								start.
							</QuestionnaireDescription>
							<Prerequisites readiness={readiness.data} refresh={refresh} />
							<QuestionnaireActions>
								<Button
									type="button"
									variant="outline"
									className="min-h-11 justify-self-start sm:min-h-8"
									onClick={() => setChose("repository")}
								>
									Back
								</Button>
								<Button
									type="button"
									className="col-start-3 min-h-11 sm:min-h-8"
									onClick={() => void refresh()}
								>
									Continue
								</Button>
							</QuestionnaireActions>
							{readiness.data.outcome === "ready" && (
								<OrganizationLink organization={readiness.data.organization} />
							)}
						</QuestionnaireItem>
					</Questionnaire>
				</>
			)}
		</Page>
	);
}

function NameOperator({
	active,
	saved,
	onSaved,
}: {
	active: boolean;
	saved: Operator | null;
	onSaved: () => Promise<void>;
}) {
	const [name, setName] = useState(saved?.name ?? "");
	const [writing, setWriting] = useState(false);
	const [error, setError] = useState<unknown>(null);
	const input = useRef<HTMLInputElement>(null);
	const uncertain =
		error !== null &&
		["connection_failed", "unknown_response", "client_failure"].includes(diagnosisOf(error).kind);

	useEffect(() => {
		if (active) input.current?.focus();
	}, [active]);

	async function inspect() {
		await onSaved();
		setError(null);
	}

	async function save() {
		if (writing || uncertain || !name.trim()) return;
		setWriting(true);
		setError(null);
		try {
			if (name.trim() !== saved?.name)
				await operator.write<Operator>("PUT", operatorPath("operator"), { name: name.trim() });
			await onSaved();
		} catch (failure) {
			setError(failure);
		} finally {
			setWriting(false);
		}
	}

	return (
		<>
			<Label htmlFor="operator-name">Your name</Label>
			<QuestionnaireInput
				id="operator-name"
				ref={input}
				value={name}
				autoComplete="name"
				disabled={writing}
				onChange={(event) => setName(event.target.value)}
				onKeyDown={(event) => {
					if (event.key === "Enter") {
						event.preventDefault();
						void save();
					}
				}}
			/>
			{error !== null && (
				<Refusal
					error={error}
					correct={() => input.current?.focus()}
					retry={() => void inspect()}
				/>
			)}
			{uncertain && (
				<Button type="button" variant="outline" onClick={() => void inspect()}>
					Inspect saved setup
				</Button>
			)}
			<QuestionnaireActions>
				<Button
					type="button"
					variant="outline"
					className="min-h-11 justify-self-start sm:min-h-8"
					disabled
				>
					Back
				</Button>
				<Button
					type="button"
					className="col-start-3 min-h-11 sm:min-h-8"
					disabled={writing || uncertain || !name.trim()}
					onClick={() => void save()}
				>
					{writing ? "Saving…" : "Continue"}
				</Button>
			</QuestionnaireActions>
		</>
	);
}

function RepositoryStep({
	organization,
	projects,
	refresh,
}: {
	organization: string;
	projects: string[];
	refresh: () => Promise<void>;
}) {
	const [way, setWay] = useState<"github" | "public">();
	return (
		<div className="grid min-w-0 gap-3 text-sm">
			{projects.length > 0 && (
				<p data-saved-projects>
					Saved: the Project {projects.join(", ")}. Add another or continue.
				</p>
			)}
			<fieldset className="flex flex-wrap gap-2">
				<legend className="sr-only">How to give a repository</legend>
				<Button
					type="button"
					variant={way === "github" ? "default" : "outline"}
					aria-pressed={way === "github"}
					onClick={() => setWay("github")}
				>
					Connect GitHub
				</Button>
				<Button
					type="button"
					variant={way === "public" ? "default" : "outline"}
					aria-pressed={way === "public"}
					onClick={() => setWay("public")}
				>
					Use a public repository
				</Button>
			</fieldset>
			{/* Both stay mounted so switching between them keeps what was entered. */}
			<div hidden={way !== "github"}>
				<ConnectGithub organization={organization} nested />
			</div>
			<div hidden={way !== "public"}>
				<ProjectForm
					organization={organization}
					purpose="public_setup"
					heading="A public repository"
					nested
					initial={A_NEW_PROJECT}
					onSaved={() => {
						setWay(undefined);
						void refresh();
					}}
				/>
			</div>
		</div>
	);
}

function Prerequisites({
	readiness,
	refresh,
}: {
	readiness: Readiness;
	refresh: () => Promise<void>;
}) {
	return (
		<div className="grid min-w-0 gap-4 text-sm">
			<dl className="grid gap-2" aria-label="Saved prerequisites">
				{readiness.saved.operator && (
					<div>
						<dt className="text-muted-foreground">Operator</dt>
						<dd>{readiness.saved.operator.name}</dd>
					</div>
				)}
				{readiness.saved.sign_ins.map((signIn) => (
					<div key={JSON.stringify([signIn.harness, signIn.method, signIn.holding])}>
						<dt className="text-muted-foreground">Saved sign-in</dt>
						<dd>
							{signIn.harness}: {signIn.method}
						</dd>
					</div>
				))}
				{readiness.saved.github_integrations.map((name) => (
					<div key={name}>
						<dt className="text-muted-foreground">GitHub Integration</dt>
						<dd>{name}</dd>
					</div>
				))}
				{readiness.saved.projects.map((name) => (
					<div key={name}>
						<dt className="text-muted-foreground">Project</dt>
						<dd>{name}</dd>
					</div>
				))}
			</dl>
			<output>
				{readiness.outcome === "unavailable"
					? "Readiness unavailable"
					: readiness.outcome === "ready"
						? "Ready to start a Session"
						: "Before starting a Session"}
			</output>
			{[...readiness.gaps, ...readiness.unavailable, ...readiness.warnings].map((diagnostic) => (
				<Diagnosis
					key={JSON.stringify(diagnostic)}
					diagnostic={diagnostic}
					retry={() => void refresh()}
				/>
			))}
			{readiness.unavailable.length > 0 && (
				<Button type="button" variant="outline" onClick={() => void refresh()}>
					Check readiness again
				</Button>
			)}
		</div>
	);
}

function OrganizationLink({ organization }: { organization: string | null }) {
	return organization === null ? (
		<Link to="/">Choose an Organization</Link>
	) : (
		<nav aria-label="Organization" className="flex flex-wrap gap-x-4 gap-y-1">
			<Link className="underline" to="/organizations/$organization" params={{ organization }}>
				Open Workspaces
			</Link>
			<Link
				className="underline"
				to="/organizations/$organization/settings/agents"
				params={{ organization }}
			>
				Agents
			</Link>
			<Link
				className="underline"
				to="/organizations/$organization/settings/repositories"
				params={{ organization }}
			>
				Repositories
			</Link>
		</nav>
	);
}
