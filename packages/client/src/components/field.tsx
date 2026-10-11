import type { ReactNode } from "react";
import { Label } from "#/components/ui/label";

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
