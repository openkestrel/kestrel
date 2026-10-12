"use client";

import { motion } from "motion/react";
import type { CSSProperties } from "react";
import { memo } from "react";
import { cn } from "#/lib/utils";

export interface TextShimmerProps {
	children: string;
	className?: string;
	duration?: number;
	spread?: number;
}

const ShimmerComponent = ({ children, className, duration = 2, spread = 2 }: TextShimmerProps) => (
	<motion.span
		animate={{ backgroundPosition: "0% center" }}
		className={cn(
			"relative inline-block bg-[length:250%_100%,auto] bg-clip-text text-transparent",
			"[--bg:linear-gradient(90deg,#0000_calc(50%-var(--spread)),var(--color-background),#0000_calc(50%+var(--spread)))] [background-repeat:no-repeat,padding-box]",
			"[background-image:var(--bg),linear-gradient(var(--color-muted-foreground),var(--color-muted-foreground))]",
			className,
		)}
		initial={{ backgroundPosition: "100% center" }}
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- a custom property is not in CSSProperties.
		style={{ "--spread": `${children.length * spread}px` } as CSSProperties}
		transition={{
			duration,
			ease: "linear",
			repeat: Number.POSITIVE_INFINITY,
		}}
	>
		{children}
	</motion.span>
);

export const Shimmer = memo(ShimmerComponent);
