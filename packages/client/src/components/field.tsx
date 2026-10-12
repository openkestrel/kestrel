import type { ReactNode } from "react";
import { NextSteps } from "#/components/refusal";
import { Label } from "#/components/ui/label";
import { diagnosisOf } from "#/operator/transport";

export function Field({
	label,
	htmlFor,
	error,
	repair,
	hint,
	children,
}: {
	label: string;
	htmlFor: string;
	error?: string;
	repair?: ReactNode;
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
			{repair}
		</div>
	);
}

export function described(id: string, hint: boolean, error: boolean): string | undefined {
	const ids = [hint ? `${id}-hint` : undefined, error ? `${id}-error` : undefined].filter(Boolean);
	return ids.length > 0 ? ids.join(" ") : undefined;
}

// Where a form's refusal stands: at the field it names, or at the top when the form has no such field.
export function placement(
	error: unknown,
	missing: Record<string, string>,
	ids: Record<string, string>,
) {
	const refused = error === null ? undefined : diagnosisOf(error);
	return {
		placed: (field: string): string | undefined =>
			refused?.field === field ? refused.message : missing[field],
		atTheTop: refused !== undefined && !Object.hasOwn(ids, refused.field ?? ""),
		// The error already stands at its field; the rest are repairs, such as declaring what is missing.
		repair: (field: string): ReactNode =>
			refused?.field === field ? (
				<NextSteps
					steps={refused.next_steps.filter(
						(step) => !(step.action === "correct_field" && step.field === field),
					)}
				/>
			) : undefined,
		correct: (field: string) => document.getElementById(ids[field] ?? "")?.focus(),
	};
}
