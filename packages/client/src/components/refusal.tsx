import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { CircleAlert } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "#/components/ui/alert";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import { Label } from "#/components/ui/label";
import { type Collected, type Fact, factsOf, type Step, stepOf } from "#/operator/diagnostic-view";
import type { Action, Diagnostic } from "#/operator/generated";
import { operator } from "#/operator/queries";
import { diagnosisOf, Refused } from "#/operator/transport";

// Without `retry`, a read again re-reads every query on the page.
export type Repairs = {
	retry?: () => void;
	correct?: (field: string) => void;
};

export function Refusal({ error, ...repairs }: { error: unknown } & Repairs) {
	const diagnostic = diagnosisOf(error);
	const status: Fact[] =
		error instanceof Refused && diagnostic.kind !== "unknown_response"
			? [["Status", String(error.status)]]
			: [];

	return (
		<Alert variant="destructive" data-diagnostic={diagnostic.kind}>
			<CircleAlert aria-hidden />
			<AlertTitle>{diagnostic.message}</AlertTitle>
			<AlertDescription>
				<Facts facts={[...status, ...factsOf(diagnostic)]} />
				<NextSteps steps={diagnostic.next_steps} {...repairs} />
			</AlertDescription>
		</Alert>
	);
}

export function Diagnosis({ diagnostic }: { diagnostic: Diagnostic }) {
	return (
		<section
			aria-label="Why it failed"
			data-diagnostic={diagnostic.kind}
			className="grid gap-1 border border-destructive/40 p-2 text-xs"
		>
			<p className="font-medium text-destructive">{diagnostic.message}</p>
			<Facts facts={factsOf(diagnostic)} />
			<NextSteps steps={diagnostic.next_steps} />
		</section>
	);
}

function Facts({ facts }: { facts: Fact[] }) {
	if (facts.length === 0) return null;
	return (
		<dl className="grid grid-cols-key-value gap-x-3">
			{facts.map(([term, detail]) => (
				<div key={`${term}:${detail}`} className="contents">
					<dt>{term}</dt>
					<dd>{detail}</dd>
				</div>
			))}
		</dl>
	);
}

export function NextSteps({ steps, ...repairs }: { steps: Action[] } & Repairs) {
	if (steps.length === 0) return null;
	return (
		<ol aria-label="Next steps" className="mt-1 grid gap-1" data-next-steps>
			{steps.map((action) => (
				<li key={JSON.stringify(action)} data-step={action.action}>
					<StepView step={stepOf(action, location.origin)} {...repairs} />
				</li>
			))}
		</ol>
	);
}

function StepView({ step, retry, correct }: { step: Step } & Repairs) {
	switch (step.kind) {
		case "link":
			return (
				<Link to={step.link.to} params={"params" in step.link ? step.link.params : {}}>
					{step.label}
				</Link>
			);
		case "reread":
			return <Reread step={step} retry={retry} />;
		case "field":
			return correct ? (
				<Button size="xs" type="button" variant="link" onClick={() => correct(step.field)}>
					{step.label}
				</Button>
			) : (
				<p>{step.label}</p>
			);
		case "guidance":
			return (
				<p>
					{step.label}
					{step.detail && <span className="block">{step.detail}</span>}
				</p>
			);
		case "write":
			return <WriteStep step={step} />;
		default: {
			const unhandled: never = step;
			throw new Error(`no such step: ${JSON.stringify(unhandled)}`);
		}
	}
}

function Reread({
	step,
	retry,
}: { step: Extract<Step, { kind: "reread" }> } & Pick<Repairs, "retry">) {
	const queryClient = useQueryClient();
	const [waiting, setWaiting] = useState(step.waitSeconds ?? 0);

	useEffect(() => {
		if (waiting <= 0) return undefined;
		const tick = setTimeout(() => setWaiting((left) => left - 1), 1000);
		return () => clearTimeout(tick);
	}, [waiting]);

	const read = () => (step.retry && retry ? retry() : void queryClient.invalidateQueries());

	return (
		<p>
			{step.detail && <span className="block">{step.detail}</span>}
			<Button size="xs" type="button" variant="outline" disabled={waiting > 0} onClick={read}>
				{waiting > 0 ? `${step.label} in ${waiting} s` : step.label}
			</Button>
		</p>
	);
}

function WriteStep({ step }: { step: Extract<Step, { kind: "write" }> }) {
	const queryClient = useQueryClient();
	const id = useId();
	const [open, setOpen] = useState(false);
	const [values, setValues] = useState<Collected>({});
	const [writing, setWriting] = useState(false);
	const [done, setDone] = useState(false);
	const [refusal, setRefusal] = useState<unknown>(null);

	const choosing = step.destructive || step.inputs.length > 0;

	async function write() {
		const { method, path, body } = step.request(values);
		setWriting(true);
		setRefusal(null);
		try {
			await operator.write(method, path, body);
			setValues({});
			setOpen(false);
			setDone(true);
			await queryClient.invalidateQueries();
		} catch (error) {
			setRefusal(error);
		} finally {
			setWriting(false);
		}
	}

	if (done) return <p data-step-done>{step.label}: done.</p>;

	if (!open && choosing) {
		return (
			<Button
				size="xs"
				type="button"
				variant={step.destructive ? "destructive" : "outline"}
				onClick={() => setOpen(true)}
			>
				{step.destructive ? `${step.label}…` : step.label}
			</Button>
		);
	}

	if (!choosing) {
		return (
			<div className="grid gap-1">
				<Button
					size="xs"
					type="button"
					variant="outline"
					disabled={writing}
					onClick={() => void write()}
				>
					{step.label}
				</Button>
				{refusal !== null && <Refusal error={refusal} />}
			</div>
		);
	}

	const complete = step.inputs.every((input) => (values[input.name] ?? "").trim() !== "");

	// A fieldset, not a form: a refusal can stand inside the form it refuses.
	return (
		<fieldset className="grid gap-1.5 border border-border p-2" data-step-form>
			<legend className="sr-only">{step.label}</legend>
			{step.destructive && step.consequence && <p data-step-consequence>{step.consequence}</p>}
			{step.inputs.map((input) => (
				<div key={input.name} className="grid gap-1">
					<Label htmlFor={`${id}-${input.name}`}>{input.label}</Label>
					<Input
						id={`${id}-${input.name}`}
						type={input.secret ? "password" : "text"}
						autoComplete="off"
						value={values[input.name] ?? ""}
						onChange={(event) => setValues({ ...values, [input.name]: event.target.value })}
					/>
				</div>
			))}
			<div className="flex gap-1">
				<Button
					size="xs"
					type="button"
					variant={step.destructive ? "destructive" : "default"}
					disabled={writing || !complete}
					onClick={() => void write()}
				>
					{step.label}
				</Button>
				<Button
					size="xs"
					type="button"
					variant="ghost"
					onClick={() => {
						setValues({});
						setOpen(false);
					}}
				>
					Cancel
				</Button>
			</div>
			{refusal !== null && <Refusal error={refusal} />}
		</fieldset>
	);
}
