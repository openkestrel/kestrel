import { cn } from "cn";
import type { ComponentProps } from "react";
import { InputGroup, InputGroupAddon, InputGroupTextarea } from "#/components/ui/input-group";

export type PromptInputProps = ComponentProps<"form">;

export const PromptInput = ({ className, children, ...props }: PromptInputProps) => (
	<form className={cn("w-full", className)} {...props}>
		<InputGroup className="overflow-hidden rounded-xl">{children}</InputGroup>
	</form>
);

export type PromptInputBodyProps = ComponentProps<"div">;

export const PromptInputBody = ({ className, ...props }: PromptInputBodyProps) => (
	<div className={cn("contents", className)} {...props} />
);

export type PromptInputTextareaProps = ComponentProps<typeof InputGroupTextarea>;

export const PromptInputTextarea = ({
	className,
	onKeyDown,
	...props
}: PromptInputTextareaProps) => (
	<InputGroupTextarea
		className={cn("field-sizing-content max-h-48 min-h-16 text-sm", className)}
		name="message"
		onKeyDown={(event) => {
			onKeyDown?.(event);
			if (
				event.defaultPrevented ||
				event.nativeEvent.isComposing ||
				event.key !== "Enter" ||
				event.shiftKey
			)
				return;
			event.preventDefault();
			const submit =
				event.currentTarget.form?.querySelector<HTMLButtonElement>('button[type="submit"]');
			if (!submit?.disabled) event.currentTarget.form?.requestSubmit();
		}}
		{...props}
	/>
);

export type PromptInputHeaderProps = Omit<ComponentProps<typeof InputGroupAddon>, "align">;

export const PromptInputHeader = ({ className, ...props }: PromptInputHeaderProps) => (
	<InputGroupAddon
		align="block-start"
		className={cn("order-first flex-wrap gap-1", className)}
		{...props}
	/>
);

export type PromptInputFooterProps = Omit<ComponentProps<typeof InputGroupAddon>, "align">;

export const PromptInputFooter = ({ className, ...props }: PromptInputFooterProps) => (
	<InputGroupAddon
		align="block-end"
		className={cn("justify-between gap-1", className)}
		{...props}
	/>
);

export type PromptInputToolsProps = ComponentProps<"div">;

export const PromptInputTools = ({ className, ...props }: PromptInputToolsProps) => (
	<div className={cn("flex min-w-0 items-center gap-1", className)} {...props} />
);
