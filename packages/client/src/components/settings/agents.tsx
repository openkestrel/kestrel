import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { described, Field, placement } from "#/components/field";
import { ModelChoice } from "#/components/model-choice";
import { Page, PageTitle } from "#/components/page";
import { Diagnosis, Refusal, RetryFallback } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { NativeSelect } from "#/components/ui/native-select";
import { Skeleton } from "#/components/ui/skeleton";
import { useDelayed } from "#/lib/use-delayed";
import {
	A_NEW_AGENT,
	type AgentDraft,
	agentDeclaration,
	agentLine,
	declareAgent,
	draftOfAgent,
	harnessChoices,
	harnessesQuery,
} from "#/operator/agents";
import type { Agent, AgentDeclaration } from "#/operator/generated";
import { agentsQuery } from "#/operator/opening";

type Editing = { declared: Agent | undefined };

export function AgentsSettings({ organization }: { organization: string }) {
	const agents = useQuery(agentsQuery(organization));
	const delayed = useDelayed(agents.isPending);
	const [editing, setEditing] = useState<Editing | null>(null);
	const [saved, setSaved] = useState<string>();

	function edit(declared: Agent | undefined) {
		setSaved(undefined);
		setEditing({ declared });
	}

	return (
		<Page>
			<PageTitle>Agents</PageTitle>
			<p className="text-muted-foreground text-sm">
				An Agent names the harness a Session runs and the defaults it starts with. Only saving here
				changes one; starting work never does.
			</p>
			<nav aria-label="Organization" className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
				<Link className="underline" to="/organizations/$organization" params={{ organization }}>
					Workspaces
				</Link>
				<Link className="underline" to="/setup" search={{ organization }}>
					Setup and readiness
				</Link>
			</nav>

			<section aria-label="Declared Agents" className="grid gap-3">
				{agents.isPending &&
					(delayed ? (
						<output data-agents-read="delayed" className="text-sm">
							Still reading the Agents; the control plane has not answered yet.
						</output>
					) : (
						<Skeleton className="h-16 w-full" />
					))}
				{agents.isError && (
					<div data-agents-read="failed" className="grid gap-1 text-sm">
						<Refusal error={agents.error} retry={() => void agents.refetch()} />
						<RetryFallback
							error={agents.error}
							retry={() => void agents.refetch()}
							label="Read the Agents again"
							disabled={agents.isFetching}
						/>
						{agents.data && (
							<p className="text-muted-foreground text-xs">Showing the Agents as last read.</p>
						)}
					</div>
				)}
				{agents.data &&
					(agents.data.length === 0 ? (
						<p data-agents-read="empty" className="text-muted-foreground text-sm">
							No Agent is declared yet. Declare one to open a Workspace with it.
						</p>
					) : (
						<ul className="grid gap-2">
							{agents.data.map((agent) => (
								<li
									key={agent.id}
									data-agent={agent.name}
									className="flex flex-wrap items-center gap-x-3 gap-y-1 border p-3 text-sm"
								>
									<span className="font-medium">{agent.name}</span>
									<span className="text-muted-foreground text-xs">{agentLine(agent)}</span>
									<Button
										className="ml-auto"
										size="sm"
										type="button"
										variant="outline"
										aria-label={`Edit ${agent.name}`}
										onClick={() => edit(agent)}
									>
										Edit
									</Button>
								</li>
							))}
						</ul>
					))}
			</section>

			{saved && <output className="text-sm">Saved the Agent {saved}.</output>}

			{editing ? (
				<AgentForm
					key={editing.declared?.id ?? "new"}
					organization={organization}
					declared={editing.declared}
					onSaved={(agent) => {
						setEditing(null);
						setSaved(agent.name);
					}}
					onCancel={() => setEditing(null)}
				/>
			) : (
				<div>
					<Button type="button" onClick={() => edit(undefined)}>
						Declare an Agent
					</Button>
				</div>
			)}
		</Page>
	);
}

const FIELD_IDS: Record<string, string> = {
	name: "agent-name",
	harness: "agent-harness",
	model: "agent-model",
	mode: "agent-mode",
	thought_level: "agent-thought-level",
};

export function AgentInputs({
	draft,
	declared,
	onChange,
	placed,
	repair,
}: {
	draft: AgentDraft;
	declared: string;
	onChange: (patch: Partial<AgentDraft>) => void;
	placed: (field: string) => string | undefined;
	repair: (field: string) => ReactNode;
}) {
	const harnesses = useQuery(harnessesQuery);
	const delayed = useDelayed(harnesses.isPending);
	const choices = harnessChoices(harnesses.data, declared);
	const chosen = harnesses.data?.find((harness) => harness.name === draft.harness);

	return (
		<>
			<Field
				label="Harness"
				htmlFor="agent-harness"
				error={placed("harness")}
				repair={repair("harness")}
				hint="The harness catalogue and the configured image decide which can run."
			>
				<NativeSelect
					id="agent-harness"
					value={draft.harness}
					onChange={(event) => onChange({ harness: event.target.value })}
					aria-invalid={placed("harness") !== undefined}
					aria-describedby={described("agent-harness", true, placed("harness") !== undefined)}
				>
					<option value="">Choose a harness</option>
					{choices.map((choice) => (
						<option key={choice.name} value={choice.name} disabled={!choice.selectable}>
							{choice.note ? `${choice.name} (${choice.note})` : choice.name}
						</option>
					))}
				</NativeSelect>
				{harnesses.isPending && (
					<output data-harnesses-read={delayed ? "delayed" : "reading"} className="text-xs">
						{delayed
							? "Still reading the harness catalogue; the control plane has not answered yet."
							: "Reading the harness catalogue…"}
					</output>
				)}
				{harnesses.isError && (
					<div data-harnesses-read="failed" className="grid gap-1 text-xs">
						<Refusal error={harnesses.error} retry={() => void harnesses.refetch()} />
						<RetryFallback
							error={harnesses.error}
							retry={() => void harnesses.refetch()}
							label="Read the harness catalogue again"
							disabled={harnesses.isFetching}
						/>
					</div>
				)}
				{chosen && (
					<p className="text-muted-foreground text-xs" data-harness-command>
						Default command <code className="font-mono text-foreground">{chosen.command}</code>; a
						harness command the control plane is configured with runs instead.
					</p>
				)}
				{chosen?.availability?.diagnostic && (
					<Diagnosis
						diagnostic={chosen.availability.diagnostic}
						retry={() => void harnesses.refetch()}
					/>
				)}
			</Field>

			<Field label="Model" htmlFor="agent-model" error={placed("model")} repair={repair("model")}>
				<ModelChoice
					id="agent-model"
					value={draft.model}
					onChange={(model) => onChange({ model })}
					aria-invalid={placed("model") !== undefined}
					aria-describedby={described("agent-model", false, placed("model") !== undefined)}
				/>
			</Field>
		</>
	);
}

function AgentForm({
	organization,
	declared,
	onSaved,
	onCancel,
}: {
	organization: string;
	declared: Agent | undefined;
	onSaved: (agent: Agent) => void;
	onCancel: () => void;
}) {
	const queryClient = useQueryClient();
	const [draft, setDraft] = useState<AgentDraft>(declared ? draftOfAgent(declared) : A_NEW_AGENT);
	const [missing, setMissing] = useState<Record<string, string>>({});

	const saving = useMutation({
		mutationFn: (declaration: AgentDeclaration) => declareAgent(organization, declaration),
		onSuccess: async (agent) => {
			await queryClient.invalidateQueries({ queryKey: agentsQuery(organization).queryKey });
			onSaved(agent);
		},
	});

	const { placed, atTheTop, repair, correct } = placement(saving.error, missing, FIELD_IDS);

	useEffect(() => {
		document.getElementById(declared ? FIELD_IDS.harness : FIELD_IDS.name)?.focus();
	}, [declared]);

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		const absent: Record<string, string> = {};
		if (draft.name.trim() === "") absent.name = "Name the Agent.";
		if (draft.harness === "") absent.harness = "Choose a harness.";
		setMissing(absent);
		saving.reset();
		if (Object.keys(absent).length > 0) return;
		saving.mutate(agentDeclaration(draft));
	};

	const change = (patch: Partial<AgentDraft>) => setDraft((held) => ({ ...held, ...patch }));
	const heading = declared ? `Edit ${declared.name}` : "Declare an Agent";

	return (
		<form onSubmit={submit} aria-label={heading} className="grid gap-4 border p-4" data-agent-form>
			<h2 className="font-semibold text-sm">{heading}</h2>
			{atTheTop && <Refusal error={saving.error} correct={correct} />}

			<Field
				label="Name"
				htmlFor="agent-name"
				error={placed("name")}
				repair={repair("name")}
				hint={
					declared
						? "An Agent keeps its name; declare another to work under a different one."
						: "What a Workspace is opened with."
				}
			>
				<Input
					id="agent-name"
					value={draft.name}
					readOnly={declared !== undefined}
					autoComplete="off"
					onChange={(event) => change({ name: event.target.value })}
					aria-invalid={placed("name") !== undefined}
					aria-describedby={described("agent-name", true, placed("name") !== undefined)}
				/>
			</Field>

			<AgentInputs
				draft={draft}
				declared={declared?.harness ?? ""}
				onChange={change}
				placed={placed}
				repair={repair}
			/>

			<Field
				label="Mode"
				htmlFor="agent-mode"
				error={placed("mode")}
				repair={repair("mode")}
				hint="Without one, the harness's default."
			>
				<Input
					id="agent-mode"
					value={draft.mode}
					autoComplete="off"
					onChange={(event) => change({ mode: event.target.value })}
					aria-invalid={placed("mode") !== undefined}
					aria-describedby={described("agent-mode", true, placed("mode") !== undefined)}
				/>
			</Field>

			<Field
				label="Thought level"
				htmlFor="agent-thought-level"
				error={placed("thought_level")}
				repair={repair("thought_level")}
				hint="Without one, the harness's default."
			>
				<Input
					id="agent-thought-level"
					value={draft.thought_level}
					autoComplete="off"
					onChange={(event) => change({ thought_level: event.target.value })}
					aria-invalid={placed("thought_level") !== undefined}
					aria-describedby={described(
						"agent-thought-level",
						true,
						placed("thought_level") !== undefined,
					)}
				/>
			</Field>

			<div className="flex gap-2">
				<Button type="submit" disabled={saving.isPending}>
					{saving.isPending ? "Saving…" : "Save Agent"}
				</Button>
				<Button type="button" variant="outline" onClick={onCancel}>
					Cancel
				</Button>
			</div>
		</form>
	);
}
