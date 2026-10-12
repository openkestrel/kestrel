import { useEffect, useId, useRef } from "react";
import {
	type ApprovalState,
	Confirmation,
	ConfirmationAccepted,
	ConfirmationAction,
	ConfirmationActions,
	ConfirmationRejected,
	ConfirmationRequest,
	ConfirmationTitle,
} from "#/components/ai-elements/confirmation";

export interface ApprovalProps {
	action: string;
	arguments: string;
	policy: string;
	mayAnswer: readonly string[];
	deadline: string;
	state: ApprovalState;
	onAnswer: (decision: "allow" | "reject") => void;
}

export function Approval({
	action,
	arguments: args,
	policy,
	mayAnswer,
	deadline,
	state,
	onAnswer,
}: ApprovalProps) {
	const title = useId();
	const result = useRef<HTMLParagraphElement>(null);
	const previous = useRef(state.status);
	useEffect(() => {
		if (previous.current !== state.status) result.current?.focus();
		previous.current = state.status;
	}, [state.status]);
	const answeredBy = state.status === "resolved" ? state.answeredBy : "";
	return (
		<section aria-labelledby={title}>
			<Confirmation state={state} role="presentation" className="min-w-0">
				<h2 id={title} className="text-sm font-medium">
					Approval: {action}
				</h2>
				<ConfirmationTitle className="block">
					<code className="block font-mono text-xs break-words">{args}</code>
				</ConfirmationTitle>
				<p className="text-xs text-muted-foreground">
					Policy <code className="font-mono">{policy}</code> · {mayAnswer.join(" or ")} may answer
				</p>
				<p
					ref={result}
					role={state.status === "refused" ? "alert" : "status"}
					tabIndex={-1}
					className="text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
				>
					<ConfirmationRequest>{`Pending · Deadline: ${deadline}`}</ConfirmationRequest>
					<ConfirmationAccepted>{`Allowed by ${answeredBy}.`}</ConfirmationAccepted>
					<ConfirmationRejected>{`Rejected by ${answeredBy}.`}</ConfirmationRejected>
					{state.status === "refused" && `Answer refused: ${state.reason}`}
					{state.status === "expired" && "Approval expired. The action was denied."}
				</p>
				<ConfirmationActions className="flex-wrap self-auto">
					<ConfirmationAction
						variant="outline"
						className="min-h-11 sm:min-h-8"
						onClick={() => onAnswer("reject")}
					>
						Reject
					</ConfirmationAction>
					<ConfirmationAction className="min-h-11 sm:min-h-8" onClick={() => onAnswer("allow")}>
						Allow
					</ConfirmationAction>
				</ConfirmationActions>
			</Confirmation>
		</section>
	);
}
