import { useEffect, useId, useRef } from "react";
import { Alert, AlertDescription } from "#/components/ui/alert";
import { Button } from "#/components/ui/button";

// These are fixture presentation states, not an operator authorization contract.
export type ApprovalState =
	| { status: "pending" }
	| { status: "resolved"; decision: "allow" | "reject"; answeredBy: string }
	| { status: "refused"; reason: string }
	| { status: "expired" };

export interface ConfirmationProps {
	action: string;
	arguments: string;
	policy: string;
	mayAnswer: readonly string[];
	deadline: string;
	state: ApprovalState;
	onAnswer: (decision: "allow" | "reject") => void;
}

export function Confirmation({
	action,
	arguments: args,
	policy,
	mayAnswer,
	deadline,
	state,
	onAnswer,
}: ConfirmationProps) {
	const title = useId();
	const result = useRef<HTMLParagraphElement>(null);
	const previous = useRef(state.status);
	useEffect(() => {
		if (previous.current !== state.status) result.current?.focus();
		previous.current = state.status;
	}, [state.status]);
	return (
		<section aria-labelledby={title}>
			<Alert role="presentation" className="flex min-w-0 flex-col gap-2">
				<h2 id={title} className="text-sm font-medium">
					Approval: {action}
				</h2>
				<AlertDescription>
					<code className="block font-mono text-xs break-words">{args}</code>
				</AlertDescription>
				<p className="text-xs text-muted-foreground">
					Policy <code className="font-mono">{policy}</code> · {mayAnswer.join(" or ")} may answer
				</p>
				<p
					ref={result}
					role={state.status === "refused" ? "alert" : "status"}
					tabIndex={-1}
					className="text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
				>
					{state.status === "pending" && `Pending · Deadline: ${deadline}`}
					{state.status === "resolved" &&
						`${state.decision === "allow" ? "Allowed" : "Rejected"} by ${state.answeredBy}.`}
					{state.status === "refused" && `Answer refused: ${state.reason}`}
					{state.status === "expired" && "Approval expired. The action was denied."}
				</p>
				{state.status === "pending" && (
					<div className="flex flex-wrap justify-end gap-2">
						<Button
							type="button"
							variant="outline"
							className="min-h-11 sm:min-h-8"
							onClick={() => onAnswer("reject")}
						>
							Reject
						</Button>
						<Button type="button" className="min-h-11 sm:min-h-8" onClick={() => onAnswer("allow")}>
							Allow
						</Button>
					</div>
				)}
			</Alert>
		</section>
	);
}
