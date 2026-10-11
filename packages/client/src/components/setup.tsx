import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { Diagnosis, Refusal, RetryFallback } from "#/components/refusal";
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
import { installQuery, readinessQuery } from "#/operator/setup";
import { diagnosisOf, operatorPath } from "#/operator/transport";

export function Setup({ organization }: { organization?: string }) {
	const install = useQuery(installQuery);
	const readiness = useQuery(readinessQuery(organization));
	const queryClient = useQueryClient();
	const [editing, setEditing] = useState(false);
	const heading = useRef<HTMLHeadingElement>(null);
	const loaded = install.isSuccess && readiness.isSuccess;
	const naming = readiness.data?.saved.operator === null || editing;

	useEffect(() => {
		if (loaded && !naming) heading.current?.focus();
	}, [naming, loaded]);

	async function refresh() {
		await queryClient.invalidateQueries();
	}

	const error = install.isError ? install.error : readiness.isError ? readiness.error : null;
	return (
		<main className="mx-auto grid min-h-dvh w-full max-w-2xl content-start gap-6 px-6 py-12 sm:py-20">
			<Link to="/" className="w-fit font-medium">
				kestrel
			</Link>
			{error !== null ? (
				<>
					<h1 className="text-xl font-semibold">Setup unavailable</h1>
					<Refusal error={error} retry={() => void refresh()} />
					<RetryFallback error={error} retry={() => void refresh()} label="Read again" />
				</>
			) : !install.isSuccess || !readiness.isSuccess ? (
				<Skeleton className="h-24 w-full" />
			) : install.data.initial_setup_complete ? (
				<>
					<h1 ref={heading} tabIndex={-1} className="text-xl font-semibold">
						Current readiness
					</h1>
					<p className="text-sm text-muted-foreground">
						Initial setup is complete. Repair any current gaps below.
					</p>
					<Prerequisites readiness={readiness.data} refresh={refresh} />
					<OrganizationLink organization={readiness.data.organization} />
				</>
			) : (
				<>
					<h1 ref={heading} tabIndex={-1} className="text-xl font-semibold">
						{naming ? "Set up kestrel" : "Continue setup"}
					</h1>
					<Questionnaire
						item={naming ? "name" : "remaining"}
						items={[{ name: "name" }, { name: "remaining" }]}
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
									await refresh();
								}}
							/>
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
									onClick={() => setEditing(true)}
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
		</main>
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
		<Link className="w-fit underline" to="/organizations/$organization" params={{ organization }}>
			Open Workspaces
		</Link>
	);
}
