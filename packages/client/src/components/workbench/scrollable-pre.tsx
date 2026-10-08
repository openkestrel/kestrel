import type { ComponentProps } from "react";

export function ScrollablePre({ className, ...props }: ComponentProps<"pre">) {
	return (
		// oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- a scrollable region must be focusable for keyboard scrolling.
		<pre className={className} tabIndex={0} {...props} />
	);
}
