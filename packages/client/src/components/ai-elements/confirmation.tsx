"use client";

import type { ComponentProps, ReactNode } from "react";
import { createContext, useContext, useMemo } from "react";
import { Alert, AlertDescription } from "#/components/ui/alert";
import { Button } from "#/components/ui/button";
import { cn } from "#/lib/utils";

// These are fixture presentation states, not an operator authorization contract.
export type ApprovalState =
	| { status: "pending" }
	| { status: "resolved"; decision: "allow" | "reject"; answeredBy: string }
	| { status: "refused"; reason: string }
	| { status: "expired" };

interface ConfirmationContextValue {
	state: ApprovalState;
}

const ConfirmationContext = createContext<ConfirmationContextValue | null>(null);

const useConfirmation = () => {
	const context = useContext(ConfirmationContext);

	if (!context) {
		throw new Error("Confirmation components must be used within Confirmation");
	}

	return context;
};

export type ConfirmationProps = ComponentProps<typeof Alert> & {
	state: ApprovalState;
};

export const Confirmation = ({ className, state, ...props }: ConfirmationProps) => {
	const contextValue = useMemo(() => ({ state }), [state]);

	return (
		<ConfirmationContext.Provider value={contextValue}>
			<Alert className={cn("flex flex-col gap-2", className)} {...props} />
		</ConfirmationContext.Provider>
	);
};

export type ConfirmationTitleProps = ComponentProps<typeof AlertDescription>;

export const ConfirmationTitle = ({ className, ...props }: ConfirmationTitleProps) => (
	<AlertDescription className={cn("inline", className)} {...props} />
);

export interface ConfirmationRequestProps {
	children?: ReactNode;
}

export const ConfirmationRequest = ({ children }: ConfirmationRequestProps) => {
	const { state } = useConfirmation();

	if (state.status !== "pending") {
		return null;
	}

	return children;
};

export interface ConfirmationAcceptedProps {
	children?: ReactNode;
}

export const ConfirmationAccepted = ({ children }: ConfirmationAcceptedProps) => {
	const { state } = useConfirmation();

	if (state.status !== "resolved" || state.decision !== "allow") {
		return null;
	}

	return children;
};

export interface ConfirmationRejectedProps {
	children?: ReactNode;
}

export const ConfirmationRejected = ({ children }: ConfirmationRejectedProps) => {
	const { state } = useConfirmation();

	if (state.status !== "resolved" || state.decision !== "reject") {
		return null;
	}

	return children;
};

export type ConfirmationActionsProps = ComponentProps<"div">;

export const ConfirmationActions = ({ className, ...props }: ConfirmationActionsProps) => {
	const { state } = useConfirmation();

	if (state.status !== "pending") {
		return null;
	}

	return (
		<div className={cn("flex items-center justify-end gap-2 self-end", className)} {...props} />
	);
};

export type ConfirmationActionProps = ComponentProps<typeof Button>;

export const ConfirmationAction = (props: ConfirmationActionProps) => (
	<Button className="h-8 px-3 text-sm" type="button" {...props} />
);
