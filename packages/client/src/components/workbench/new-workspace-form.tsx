import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { ChevronRight } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { NativeSelect } from "#/components/ui/native-select";
import { Skeleton } from "#/components/ui/skeleton";
import { Textarea } from "#/components/ui/textarea";
import {
	AN_EMPTY_DRAFT,
	draftOf,
	holdDraft,
	type NewWorkspaceDraft,
} from "#/lib/new-workspace-draft";
import type { WorkspaceDeclaration } from "#/operator/generated";
import {
	agentsQuery,
	newWorkspaceDeclaration,
	openWorkspace,
	profilesQuery,
	projectsQuery,
	resolvedBranch,
	resolvedEnvironment,
	resolvedModel,
	resolvedRepositories,
} from "#/operator/opening";
import { participant } from "#/operator/participant";
import { queueQuery } from "#/operator/queries";
import { openingQueueLine } from "#/operator/queue-line";
import { Refused } from "#/operator/transport";
import { PaneHeading } from "./workbench";

const PLACED_FIELDS = new Set([
	"project",
	"agent",
	"profile",
	"branch",
	"model",
	"brief",
	"participant",
]);

const OPTION_FIELDS = new Set(["profile", "branch", "model"]);

export function NewWorkspaceForm({ organization }: { organization: string }) {
	const navigate = useNavigate();
	const projects = useQuery(projectsQuery(organization));
	const agents = useQuery(agentsQuery(organization));
	const profiles = useQuery(profilesQuery(organization));
	const queue = useQuery(queueQuery(organization));

	const [held, setHeld] = useState<NewWorkspaceDraft | undefined>(() => draftOf(organization));
	const [name, setName] = useState(() => participant.name() ?? "");
	const [missing, setMissing] = useState<Record<string, string>>({});

	// Derived rather than set in an effect, so only what the person changes is held in the store.
	const preselected: NewWorkspaceDraft | undefined =
		projects.data && agents.data
			? {
					...AN_EMPTY_DRAFT,
					project: projects.data.length === 1 ? projects.data[0].name : "",
					agent: agents.data.length === 1 ? agents.data[0].name : "",
				}
			: undefined;
	const draft = held ?? preselected;

	function hold(patch: Partial<NewWorkspaceDraft>) {
		setHeld((previous) => {
			const next = { ...(previous ?? draft ?? AN_EMPTY_DRAFT), ...patch };
			holdDraft(organization, next);
			return next;
		});
	}

	const opening = useMutation({
		mutationFn: (declaration: WorkspaceDeclaration) => openWorkspace(organization, declaration),
		onSuccess: (opened) => {
			participant.remember(name);
			void navigate({
				to: "/organizations/$organization/workspaces/$workspace",
				params: { organization, workspace: opened.workspace.name },
			});
		},
		onError: (error) => {
			if (error instanceof Refused && OPTION_FIELDS.has(error.field ?? "")) {
				hold({ options: true });
			}
		},
	});

	const refused = opening.error instanceof Refused ? opening.error : undefined;
	const placed = (field: string): string | undefined =>
		refused?.field === field ? refused.message : missing[field];
	const at_the_top =
		opening.error !== undefined &&
		!(opening.error instanceof Refused && PLACED_FIELDS.has(opening.error.field ?? ""));

	if (draft === undefined || projects.isPending || agents.isPending || profiles.isPending) {
		return (
			<>
				<PaneHeading>New Workspace</PaneHeading>
				<Skeleton className="m-4 h-8" />
			</>
		);
	}

	if (projects.isError || agents.isError || profiles.isError) {
		return (
			<>
				<PaneHeading>New Workspace</PaneHeading>
				<div className="p-4">
					<Refusal error={projects.error ?? agents.error ?? profiles.error} />
				</div>
			</>
		);
	}

	const project = projects.data.find((candidate) => candidate.name === draft.project);
	const agent = agents.data.find((candidate) => candidate.name === draft.agent);
	const show_name = name.trim() === "" || refused?.field === "participant";

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		const declaration = newWorkspaceDeclaration(draft, name);
		const absent: Record<string, string> = {};
		if (draft.project === "") absent.project = "Choose a Project.";
		if (draft.agent === "") absent.agent = "Choose an Agent.";
		if (declaration.brief !== null && name.trim() === "") {
			absent.participant = "Your name is needed before sending.";
		}
		setMissing(absent);
		if (Object.keys(absent).length > 0) return;

		opening.mutate(declaration);
	};

	return (
		<>
			<PaneHeading>New Workspace</PaneHeading>
			<form onSubmit={submit} className="grid gap-4 p-4">
				{at_the_top && <Refusal error={opening.error} />}

				<Field
					label="Project"
					htmlFor="new-workspace-project"
					error={placed("project")}
					hint="Its repositories and base branch are the work's."
				>
					<NativeSelect
						id="new-workspace-project"
						value={draft.project}
						onChange={(event) => hold({ project: event.target.value })}
						aria-invalid={placed("project") !== undefined}
						aria-describedby={described(
							"new-workspace-project",
							true,
							placed("project") !== undefined,
						)}
					>
						<option value="">Choose a Project</option>
						{projects.data.map((candidate) => (
							<option key={candidate.id} value={candidate.name}>
								{candidate.name}
							</option>
						))}
					</NativeSelect>
				</Field>

				<Field
					label="Agent"
					htmlFor="new-workspace-agent"
					error={placed("agent")}
					hint="Its harness and model are the Session's."
				>
					<NativeSelect
						id="new-workspace-agent"
						value={draft.agent}
						onChange={(event) => hold({ agent: event.target.value })}
						aria-invalid={placed("agent") !== undefined}
						aria-describedby={described("new-workspace-agent", true, placed("agent") !== undefined)}
					>
						<option value="">Choose an Agent</option>
						{agents.data.map((candidate) => (
							<option key={candidate.id} value={candidate.name}>
								{candidate.name}
							</option>
						))}
					</NativeSelect>
				</Field>

				<Collapsible open={draft.options} onOpenChange={(options) => hold({ options })}>
					<CollapsibleTrigger render={<Button variant="outline" size="sm" />}>
						<ChevronRight aria-hidden />
						Options
					</CollapsibleTrigger>
					<CollapsibleContent>
						<div className="grid gap-3 pt-3">
							<Field
								label="Model"
								htmlFor="new-workspace-model"
								error={placed("model")}
								hint="Without one, the Agent's, then the harness's default."
							>
								<Input
									id="new-workspace-model"
									value={draft.model}
									onChange={(event) => hold({ model: event.target.value })}
									aria-invalid={placed("model") !== undefined}
									aria-describedby={described(
										"new-workspace-model",
										true,
										placed("model") !== undefined,
									)}
								/>
							</Field>

							<Field
								label="Subscription Profile"
								htmlFor="new-workspace-profile"
								error={placed("profile")}
								hint="The access the harness runs under."
							>
								<NativeSelect
									id="new-workspace-profile"
									value={draft.profile}
									onChange={(event) => hold({ profile: event.target.value })}
									aria-invalid={placed("profile") !== undefined}
									aria-describedby={described(
										"new-workspace-profile",
										true,
										placed("profile") !== undefined,
									)}
								>
									<option value="">None</option>
									{profiles.data.map((profile) => (
										<option key={profile.id} value={profile.name}>
											{profile.name}
										</option>
									))}
								</NativeSelect>
							</Field>

							<Field
								label="Branch"
								htmlFor="new-workspace-branch"
								error={placed("branch")}
								hint="Without one, the Project's base branch."
							>
								<Input
									id="new-workspace-branch"
									value={draft.branch}
									onChange={(event) => hold({ branch: event.target.value })}
									placeholder="kestrel/<workspace>"
									aria-invalid={placed("branch") !== undefined}
									aria-describedby={described(
										"new-workspace-branch",
										true,
										placed("branch") !== undefined,
									)}
								/>
							</Field>
						</div>
					</CollapsibleContent>
				</Collapsible>

				<dl className="grid grid-cols-key-value gap-x-3 gap-y-1 border p-2.5 text-xs">
					<dt className="text-muted-foreground">Repositories</dt>
					<dd>{resolvedRepositories(project)}</dd>
					<dt className="text-muted-foreground">Base branch</dt>
					<dd>{resolvedBranch(draft.branch, project)}</dd>
					<dt className="text-muted-foreground">Harness</dt>
					<dd>{agent?.harness ?? ""}</dd>
					<dt className="text-muted-foreground">Model</dt>
					<dd>{resolvedModel(draft.model, agent)}</dd>
					<dt className="text-muted-foreground">Environment</dt>
					<dd>{queue.data ? resolvedEnvironment(queue.data.work_role) : "Reading the queue…"}</dd>
					{draft.continues !== "" && (
						<>
							<dt className="text-muted-foreground">Continues</dt>
							<dd data-continues>{draft.continues}</dd>
						</>
					)}
				</dl>

				{queue.data ? (
					<output data-live className="text-muted-foreground text-xs">
						{openingQueueLine(queue.data, draft.brief.trim() !== "")}
					</output>
				) : null}

				<Field
					label="Brief"
					htmlFor="new-workspace-brief"
					error={placed("brief")}
					hint="Type, paste, or drop a file; it is read here and never uploaded. Empty is fine: the Session then waits for your first message."
				>
					<Textarea
						id="new-workspace-brief"
						value={draft.brief}
						onChange={(event) => hold({ brief: event.target.value })}
						onDragOver={(event) => event.preventDefault()}
						onDrop={(event) => {
							event.preventDefault();
							const dropped = event.dataTransfer.files[0];
							if (dropped) void dropped.text().then((text) => hold({ brief: text }));
						}}
						aria-invalid={placed("brief") !== undefined}
						aria-describedby={described("new-workspace-brief", true, placed("brief") !== undefined)}
					/>
				</Field>

				{show_name && (
					<Field
						label="Your name"
						htmlFor="new-workspace-name"
						error={placed("participant")}
						hint="Remembered in this browser; your Brief is written under it."
					>
						<Input
							id="new-workspace-name"
							value={name}
							onChange={(event) => setName(event.target.value)}
							autoComplete="nickname"
							aria-invalid={placed("participant") !== undefined}
							aria-describedby={described(
								"new-workspace-name",
								true,
								placed("participant") !== undefined,
							)}
						/>
					</Field>
				)}

				<div>
					<Button type="submit" disabled={opening.isPending}>
						{opening.isPending ? "Opening…" : "Open Workspace"}
					</Button>
				</div>
			</form>
		</>
	);
}

function Field({
	label,
	htmlFor,
	error,
	hint,
	children,
}: {
	label: string;
	htmlFor: string;
	error?: string;
	hint?: string;
	children: ReactNode;
}) {
	return (
		<div className="grid gap-1.5">
			<Label htmlFor={htmlFor}>{label}</Label>
			{children}
			{hint && (
				<p id={`${htmlFor}-hint`} className="text-muted-foreground text-xs">
					{hint}
				</p>
			)}
			{error && (
				<p id={`${htmlFor}-error`} role="alert" className="text-destructive text-xs">
					{error}
				</p>
			)}
		</div>
	);
}

function described(id: string, hint: boolean, error: boolean): string | undefined {
	const ids = [hint ? `${id}-hint` : undefined, error ? `${id}-error` : undefined].filter(Boolean);
	return ids.length > 0 ? ids.join(" ") : undefined;
}
