import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useEffect, useState } from "react";
import { described, Field, placement } from "#/components/field";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { NativeSelect } from "#/components/ui/native-select";
import type {
	Integration,
	Project,
	RepositoryPurpose,
	RepositoryResolution,
} from "#/operator/generated";
import { projectsQuery } from "#/operator/opening";
import {
	declareProject,
	type ProjectDraft,
	projectDeclaration,
	type RepositoryRow,
	resolutionRequest,
	resolveRepositories,
} from "#/operator/repositories";

const WORK_BRANCH =
	"Each Workspace works on a branch of its own, which kestrel names when it opens; this is only where that branch starts.";

const IDS: Record<string, string> = {
	name: "project-name",
	repositories: "project-repository-0",
	integrations: "project-integration-0",
	branch: "project-branch",
};

const NO_AUTHORITIES: Integration[] = [];

// The review is of what was resolved, so it holds the draft it was resolved from.
type Reviewed = { draft: ProjectDraft; resolution: RepositoryResolution };

export function ProjectForm({
	organization,
	purpose,
	initial,
	authorities = NO_AUTHORITIES,
	declared = false,
	nested = false,
	heading,
	onSaved,
	onCancel,
}: {
	organization: string;
	purpose: RepositoryPurpose;
	initial: ProjectDraft;
	authorities?: Integration[];
	declared?: boolean;
	// Inside another form a form would submit that one, so this is a fieldset whose Review is a plain button.
	nested?: boolean;
	heading: string;
	onSaved: (project: Project) => void;
	onCancel?: () => void;
}) {
	const queryClient = useQueryClient();
	const [draft, setDraft] = useState(initial);
	const [missing, setMissing] = useState<Record<string, string>>({});
	const [reviewed, setReviewed] = useState<Reviewed | null>(null);

	const resolving = useMutation({
		mutationFn: (held: ProjectDraft) =>
			resolveRepositories(resolutionRequest(held, organization, purpose)),
		onSuccess: (resolution, held) => setReviewed({ draft: held, resolution }),
	});

	const saving = useMutation({
		mutationFn: (held: ProjectDraft) => declareProject(organization, projectDeclaration(held)),
		onSuccess: async (project) => {
			await queryClient.invalidateQueries({ queryKey: projectsQuery(organization).queryKey });
			await queryClient.invalidateQueries({ queryKey: ["readiness"] });
			onSaved(project);
		},
		onError: () => setReviewed(null),
	});

	const refusal = saving.error ?? resolving.error;
	const { placed, atTheTop, repair, correct } = placement(refusal, missing, IDS);
	const wrong =
		reviewed === null ? Object.keys(IDS).find((name) => placed(name) !== undefined) : undefined;

	useEffect(() => {
		if (wrong) document.getElementById(IDS[wrong])?.focus();
	}, [wrong]);

	const change = (patch: Partial<ProjectDraft>) => setDraft((held) => ({ ...held, ...patch }));
	const changeRow = (index: number, patch: Partial<RepositoryRow>) =>
		change({
			repositories: draft.repositories.map((row, at) =>
				at === index ? { ...row, ...patch } : row,
			),
		});

	const review = () => {
		const absent: Record<string, string> = {};
		if (draft.name.trim() === "") absent.name = "Name the Project.";
		if (draft.repositories.every(({ address }) => address.trim() === ""))
			absent.repositories = "Give a repository.";
		if (draft.branch.trim() === "") absent.branch = "Name the branch a Workspace checks out.";
		setMissing(absent);
		saving.reset();
		resolving.reset();
		if (Object.keys(absent).length === 0) resolving.mutate(draft);
	};

	if (reviewed !== null)
		return (
			<section
				aria-label={`Review ${heading}`}
				className="grid gap-4 border p-4"
				data-project-review
			>
				<h2 className="font-semibold text-sm">Review the Project {reviewed.draft.name.trim()}</h2>
				<dl className="grid gap-3 text-sm">
					{reviewed.resolution.repositories.map((repository) => (
						<div key={repository.given} className="grid gap-0.5" data-resolved={repository.given}>
							<dt className="text-muted-foreground">Repository</dt>
							<dd className="break-all font-mono" data-address>
								{repository.address}
							</dd>
							<dd className="text-muted-foreground text-xs">
								Checked out into <code className="font-mono">{repository.checkout_directory}</code>
							</dd>
							<dd data-authority>
								{repository.integration === null
									? "No GitHub App authority: a Session can read a public repository but kestrel lends it nothing to push with."
									: `Reaches GitHub as the Integration ${repository.integration}.`}
							</dd>
						</div>
					))}
					<div className="grid gap-0.5">
						<dt className="text-muted-foreground">Checkout base branch</dt>
						<dd className="font-mono" data-base-branch>
							{reviewed.draft.branch.trim()}
						</dd>
						<dd className="text-muted-foreground text-xs">{WORK_BRANCH}</dd>
					</div>
				</dl>
				{reviewed.resolution.warnings.map((warning) => (
					<p key={warning.code} className="text-sm" data-warning={warning.code}>
						{warning.message}
					</p>
				))}
				<p className="text-muted-foreground text-xs" data-save-claim>
					{reviewed.resolution.repositories.some(({ integration }) => integration !== null)
						? "Saving first asks the selected App for each repository and the branch; a passed check says the App reached them, not that a checkout will succeed."
						: "Saving accepts the address's form only; it does not reach the repository or prove a checkout."}
				</p>
				<div className="flex gap-2">
					<Button
						type="button"
						disabled={saving.isPending}
						onClick={() => saving.mutate(reviewed.draft)}
					>
						{saving.isPending ? "Saving…" : "Save Project"}
					</Button>
					<Button
						type="button"
						variant="outline"
						disabled={saving.isPending}
						onClick={() => setReviewed(null)}
					>
						Back
					</Button>
				</div>
			</section>
		);

	const Shell = nested ? "fieldset" : "form";
	return (
		<Shell
			onSubmit={(event: FormEvent) => {
				event.preventDefault();
				review();
			}}
			aria-label={heading}
			className="grid gap-4 border p-4"
			data-project-form
		>
			<h2 className="font-semibold text-sm">{heading}</h2>
			{atTheTop && <Refusal error={refusal} correct={correct} />}

			<Field
				label="Project name"
				htmlFor={IDS.name}
				error={placed("name")}
				repair={repair("name")}
				hint={
					declared
						? "A Project keeps its name; saving replaces its repositories and branch."
						: "What a Workspace is opened against."
				}
			>
				<Input
					id={IDS.name}
					value={draft.name}
					readOnly={declared}
					autoComplete="off"
					onChange={(event) => change({ name: event.target.value })}
					aria-invalid={placed("name") !== undefined}
					aria-describedby={described(IDS.name, true, placed("name") !== undefined)}
				/>
			</Field>

			{draft.repositories.map((row, index) => {
				const address = `project-repository-${index}`;
				const authority = `project-integration-${index}`;
				const field = index === 0 ? "repositories" : "";
				const selection = index === 0 ? "integrations" : "";
				return (
					<fieldset key={address} className="grid gap-3" data-repository-row={index}>
						<legend className="sr-only">Repository {index + 1}</legend>
						<Field
							label={index === 0 ? "Repository" : `Repository ${index + 1}`}
							htmlFor={address}
							error={placed(field)}
							repair={repair(field)}
							hint={
								purpose === "public_setup"
									? "An HTTPS address or owner/repo on GitHub."
									: "An HTTPS, SSH or git URL, user@host:path, a path on the Instance, or owner/repo on GitHub."
							}
						>
							<Input
								id={address}
								value={row.address}
								autoComplete="off"
								spellCheck={false}
								onChange={(event) => changeRow(index, { address: event.target.value })}
								aria-invalid={placed(field) !== undefined}
								aria-describedby={described(address, true, placed(field) !== undefined)}
							/>
						</Field>
						{purpose === "declaration" && (
							<Field
								label="Reach GitHub as"
								htmlFor={authority}
								error={placed(selection)}
								repair={repair(selection)}
								hint={
									authorities.length === 0
										? "No GitHub Integration lends authority yet; connect GitHub for a Session to push."
										: "An Integration lends the App's authority only to the repository it watches."
								}
							>
								<NativeSelect
									id={authority}
									value={row.integration}
									onChange={(event) => {
										const chosen = authorities.find(({ name }) => name === event.target.value);
										changeRow(index, {
											integration: event.target.value,
											...(row.address.trim() === "" &&
												chosen?.repository && { address: chosen.repository }),
										});
									}}
									aria-invalid={placed(selection) !== undefined}
									aria-describedby={described(authority, true, placed(selection) !== undefined)}
								>
									<option value="">No Integration</option>
									{authorities.map((integration) => (
										<option key={integration.id} value={integration.name}>
											{integration.repository
												? `${integration.name} (${integration.repository})`
												: integration.name}
										</option>
									))}
								</NativeSelect>
							</Field>
						)}
						{draft.repositories.length > 1 && (
							<Button
								type="button"
								size="xs"
								variant="ghost"
								className="justify-self-start"
								onClick={() =>
									change({ repositories: draft.repositories.filter((_, at) => at !== index) })
								}
							>
								Remove repository {index + 1}
							</Button>
						)}
					</fieldset>
				);
			})}
			{purpose === "declaration" && (
				<Button
					type="button"
					size="sm"
					variant="outline"
					className="justify-self-start"
					onClick={() =>
						change({ repositories: [...draft.repositories, { address: "", integration: "" }] })
					}
				>
					Add a repository
				</Button>
			)}

			<Field
				label="Checkout base branch"
				htmlFor={IDS.branch}
				error={placed("branch")}
				repair={repair("branch")}
				hint={WORK_BRANCH}
			>
				<Input
					id={IDS.branch}
					value={draft.branch}
					autoComplete="off"
					spellCheck={false}
					onChange={(event) => change({ branch: event.target.value })}
					aria-invalid={placed("branch") !== undefined}
					aria-describedby={described(IDS.branch, true, placed("branch") !== undefined)}
				/>
			</Field>

			<div className="flex gap-2">
				<Button
					type={nested ? "button" : "submit"}
					disabled={resolving.isPending}
					onClick={nested ? review : undefined}
				>
					{resolving.isPending ? "Resolving…" : "Review"}
				</Button>
				{onCancel && (
					<Button type="button" variant="outline" onClick={onCancel}>
						Cancel
					</Button>
				)}
			</div>
		</Shell>
	);
}
