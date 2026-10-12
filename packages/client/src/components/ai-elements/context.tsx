"use client";

import { cn } from "cn";
import type { ComponentProps } from "react";
import { createContext, useContext, useMemo } from "react";
import { Button } from "#/components/ui/button";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "#/components/ui/hover-card";
import { Progress } from "#/components/ui/progress";
import type { Usage } from "#/operator/generated";

const PERCENT_MAX = 100;
const ICON_RADIUS = 10;
const ICON_VIEWBOX = 24;
const ICON_CENTER = 12;
const ICON_STROKE_WIDTH = 2;

interface ContextSchema {
	usedTokens: number;
	maxTokens: number;
	usage?: Usage;
}

const ContextContext = createContext<ContextSchema | null>(null);

const useContextValue = () => {
	const context = useContext(ContextContext);

	if (!context) {
		throw new Error("Context components must be used within Context");
	}

	return context;
};

export type ContextProps = ComponentProps<typeof HoverCard> & ContextSchema;

export const Context = ({ usedTokens, maxTokens, usage, ...props }: ContextProps) => {
	const contextValue = useMemo(
		() => ({ maxTokens, usage, usedTokens }),
		[maxTokens, usage, usedTokens],
	);

	return (
		<ContextContext.Provider value={contextValue}>
			<HoverCard {...props} />
		</ContextContext.Provider>
	);
};

const ContextIcon = () => {
	const { usedTokens, maxTokens } = useContextValue();
	const circumference = 2 * Math.PI * ICON_RADIUS;
	const usedPercent = maxTokens > 0 ? usedTokens / maxTokens : 0;
	const dashOffset = circumference * (1 - usedPercent);

	return (
		<svg
			aria-hidden="true"
			height="20"
			className="text-current"
			viewBox={`0 0 ${ICON_VIEWBOX} ${ICON_VIEWBOX}`}
			width="20"
		>
			<circle
				cx={ICON_CENTER}
				cy={ICON_CENTER}
				fill="none"
				opacity="0.25"
				r={ICON_RADIUS}
				stroke="currentColor"
				strokeWidth={ICON_STROKE_WIDTH}
			/>
			<circle
				cx={ICON_CENTER}
				cy={ICON_CENTER}
				fill="none"
				opacity="0.7"
				r={ICON_RADIUS}
				stroke="currentColor"
				strokeDasharray={`${circumference} ${circumference}`}
				strokeDashoffset={dashOffset}
				strokeLinecap="round"
				strokeWidth={ICON_STROKE_WIDTH}
				className="origin-center -rotate-90"
			/>
		</svg>
	);
};

export type ContextTriggerProps = ComponentProps<typeof Button>;

export const ContextTrigger = ({ children, ...props }: ContextTriggerProps) => {
	const { usedTokens, maxTokens } = useContextValue();
	const usedPercent = maxTokens > 0 ? usedTokens / maxTokens : 0;
	const renderedPercent = new Intl.NumberFormat("en-US", {
		maximumFractionDigits: 1,
		style: "percent",
	}).format(usedPercent);

	return (
		<HoverCardTrigger
			delay={0}
			closeDelay={0}
			render={<Button type="button" variant="ghost" {...props} />}
		>
			{children ?? (
				<>
					<span className="font-medium text-muted-foreground">{renderedPercent}</span>
					<ContextIcon />
				</>
			)}
		</HoverCardTrigger>
	);
};

export type ContextContentProps = ComponentProps<typeof HoverCardContent>;

export const ContextContent = ({ className, ...props }: ContextContentProps) => (
	<HoverCardContent className={cn("min-w-60 divide-y overflow-hidden p-0", className)} {...props} />
);

export type ContextContentHeaderProps = ComponentProps<"div">;

export const ContextContentHeader = ({
	children,
	className,
	...props
}: ContextContentHeaderProps) => {
	const { usedTokens, maxTokens } = useContextValue();
	const usedPercent = maxTokens > 0 ? usedTokens / maxTokens : 0;
	const displayPct = new Intl.NumberFormat("en-US", {
		maximumFractionDigits: 1,
		style: "percent",
	}).format(usedPercent);
	const used = new Intl.NumberFormat("en-US", {
		maximumFractionDigits: 0,
	}).format(usedTokens);
	const total = new Intl.NumberFormat("en-US", {
		maximumFractionDigits: 0,
	}).format(maxTokens);

	return (
		<div className={cn("w-full space-y-2 p-3", className)} {...props}>
			{children ?? (
				<>
					<div className="flex items-center justify-between gap-3 text-xs">
						<p>{displayPct}</p>
						<p className="font-mono text-muted-foreground">
							{used} / {total}
						</p>
					</div>
					<div className="space-y-2">
						<Progress
							aria-label="Context used"
							value={Math.min(usedPercent * PERCENT_MAX, PERCENT_MAX)}
						/>
					</div>
				</>
			)}
		</div>
	);
};

export type ContextContentBodyProps = ComponentProps<"div">;

export const ContextContentBody = ({ children, className, ...props }: ContextContentBodyProps) => (
	<div className={cn("w-full p-3", className)} {...props}>
		{children}
	</div>
);

export type ContextContentFooterProps = ComponentProps<"div">;

export const ContextContentFooter = ({
	children,
	className,
	...props
}: ContextContentFooterProps) => {
	const { usage } = useContextValue();
	return (
		<div className={cn("w-full bg-muted/50 p-3", className)} {...props}>
			{children ??
				(usage?.cost
					? `${usage.cost.amount.toFixed(2)} ${usage.cost.currency}`
					: "Cost not reported")}
		</div>
	);
};
