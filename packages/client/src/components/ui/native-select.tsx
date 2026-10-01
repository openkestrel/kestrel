import { cn } from "cn";
import type * as React from "react";

function NativeSelect({ className, children, ...props }: React.ComponentProps<"select">) {
	return (
		<select
			data-slot="native-select"
			className={cn(
				"h-8 w-full min-w-0 rounded-none border border-input bg-transparent px-2 text-xs transition-colors outline-none focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-1 aria-invalid:ring-destructive/20",
				className,
			)}
			{...props}
		>
			{children}
		</select>
	);
}

export { NativeSelect };
