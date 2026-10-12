"use client";

import { AlertTriangleIcon, ChevronDownIcon } from "lucide-react";
import type { ComponentProps } from "react";
import { createContext, memo, useCallback, useContext, useMemo, useState } from "react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { cn } from "#/lib/utils";

// Regex patterns for parsing stack traces
const STACK_FRAME_WITH_PARENS_REGEX = /^at\s+(.+?)\s+\((.+):(\d+):(\d+)\)$/;
const STACK_FRAME_WITHOUT_FN_REGEX = /^at\s+(.+):(\d+):(\d+)$/;
const ERROR_TYPE_REGEX = /^(\w+Error|Error):\s*(.*)$/;
const AT_PREFIX_REGEX = /^at\s+/;

interface StackFrame {
	raw: string;
	functionName: string | null;
	filePath: string | null;
	lineNumber: number | null;
	columnNumber: number | null;
	isInternal: boolean;
}

interface ParsedStackTrace {
	errorType: string | null;
	errorMessage: string;
	frames: StackFrame[];
	raw: string;
}

interface StackTraceContextValue {
	trace: ParsedStackTrace;
	raw: string;
	isOpen: boolean;
	onFilePathClick?: (filePath: string, line?: number, column?: number) => void;
}

const StackTraceContext = createContext<StackTraceContextValue | null>(null);

const useStackTrace = () => {
	const context = useContext(StackTraceContext);
	if (!context) {
		throw new Error("StackTrace components must be used within StackTrace");
	}
	return context;
};

const located = (
	raw: string,
	functionName: string | null = null,
	filePath = "",
	line = "",
	column = "",
): StackFrame => ({
	columnNumber: column ? Number.parseInt(column, 10) : null,
	filePath,
	functionName,
	isInternal:
		filePath.includes("node_modules") ||
		filePath.startsWith("node:") ||
		filePath.includes("internal/"),
	lineNumber: line ? Number.parseInt(line, 10) : null,
	raw,
});

const parseStackFrame = (line: string): StackFrame => {
	const trimmed = line.trim();

	// Pattern: at functionName (filePath:line:column)
	const withParensMatch = trimmed.match(STACK_FRAME_WITH_PARENS_REGEX);
	if (withParensMatch) {
		const [, functionName, filePath, lineNum, colNum] = withParensMatch;
		return located(trimmed, functionName, filePath, lineNum, colNum);
	}

	// Pattern: at filePath:line:column (no function name)
	const withoutFnMatch = trimmed.match(STACK_FRAME_WITHOUT_FN_REGEX);
	if (withoutFnMatch) {
		const [, filePath, lineNum, colNum] = withoutFnMatch;
		return located(trimmed, null, filePath, lineNum, colNum);
	}

	// Fallback: unparseable line
	return {
		columnNumber: null,
		filePath: null,
		functionName: null,
		isInternal: trimmed.includes("node_modules") || trimmed.includes("node:"),
		lineNumber: null,
		raw: trimmed,
	};
};

const parseStackTrace = (trace: string): ParsedStackTrace => {
	const lines = trace.split("\n").filter((line) => line.trim());

	if (lines.length === 0) {
		return {
			errorMessage: trace,
			errorType: null,
			frames: [],
			raw: trace,
		};
	}

	const firstLine = lines[0].trim();
	let errorType: string | null = null;
	let errorMessage = firstLine;

	// Try to extract error type from "ErrorType: message" format
	const errorMatch = firstLine.match(ERROR_TYPE_REGEX);
	if (errorMatch) {
		const [, type, msg] = errorMatch;
		errorType = type;
		errorMessage = msg || "";
	}

	// Parse stack frames (lines starting with "at")
	const frames = lines
		.slice(1)
		.filter((line) => line.trim().startsWith("at "))
		.map(parseStackFrame);

	return {
		errorMessage,
		errorType,
		frames,
		raw: trace,
	};
};

export type StackTraceProps = Omit<ComponentProps<typeof Collapsible>, "onOpenChange"> & {
	trace: string;
	onOpenChange?: (open: boolean) => void;
	onFilePathClick?: (filePath: string, line?: number, column?: number) => void;
};

// One Collapsible root, so Base UI ties the header's button to the frames it opens.
export const StackTrace = memo(
	({
		trace,
		className,
		open,
		defaultOpen = false,
		onOpenChange,
		onFilePathClick,
		children,
		...props
	}: StackTraceProps) => {
		const [uncontrolled, setUncontrolled] = useState(defaultOpen);
		const isOpen = open ?? uncontrolled;
		const setIsOpen = useCallback(
			(next: boolean) => {
				setUncontrolled(next);
				onOpenChange?.(next);
			},
			[onOpenChange],
		);

		const parsedTrace = useMemo(() => parseStackTrace(trace), [trace]);

		const contextValue = useMemo(
			() => ({ isOpen, onFilePathClick, raw: trace, trace: parsedTrace }),
			[parsedTrace, trace, isOpen, onFilePathClick],
		);

		return (
			<StackTraceContext.Provider value={contextValue}>
				<Collapsible
					className={cn("w-full overflow-hidden border bg-background font-mono text-xs", className)}
					onOpenChange={setIsOpen}
					open={isOpen}
					{...props}
				>
					{children}
				</Collapsible>
			</StackTraceContext.Provider>
		);
	},
);

export type StackTraceHeaderProps = ComponentProps<typeof CollapsibleTrigger>;

export const StackTraceHeader = memo(({ className, children, ...props }: StackTraceHeaderProps) => (
	<CollapsibleTrigger
		className={cn(
			"flex w-full cursor-pointer items-center gap-3 p-2 text-left transition-colors hover:bg-muted/50",
			className,
		)}
		{...props}
	>
		{children}
	</CollapsibleTrigger>
));

export type StackTraceErrorProps = ComponentProps<"span">;

export const StackTraceError = memo(({ className, children, ...props }: StackTraceErrorProps) => (
	<span
		className={cn("flex min-w-0 flex-1 items-center gap-2 overflow-hidden", className)}
		{...props}
	>
		<AlertTriangleIcon aria-hidden className="size-4 shrink-0 text-destructive" />
		{children}
	</span>
));

export type StackTraceErrorTypeProps = ComponentProps<"span">;

export const StackTraceErrorType = memo(
	({ className, children, ...props }: StackTraceErrorTypeProps) => {
		const { trace } = useStackTrace();

		return (
			<span className={cn("shrink-0 font-semibold text-destructive", className)} {...props}>
				{children ?? trace.errorType}
			</span>
		);
	},
);

export type StackTraceErrorMessageProps = ComponentProps<"span">;

export const StackTraceErrorMessage = memo(
	({ className, children, ...props }: StackTraceErrorMessageProps) => {
		const { trace } = useStackTrace();

		return (
			<span className={cn("truncate text-foreground", className)} {...props}>
				{children ?? trace.errorMessage}
			</span>
		);
	},
);

export type StackTraceExpandButtonProps = ComponentProps<"span">;

export const StackTraceExpandButton = memo(
	({ className, ...props }: StackTraceExpandButtonProps) => {
		const { isOpen } = useStackTrace();

		return (
			<span
				aria-hidden
				className={cn("flex size-5 items-center justify-center", className)}
				{...props}
			>
				<ChevronDownIcon
					className={cn(
						"size-4 text-muted-foreground transition-transform",
						isOpen ? "rotate-180" : "rotate-0",
					)}
				/>
			</span>
		);
	},
);

export type StackTraceContentProps = ComponentProps<typeof CollapsibleContent>;

export const StackTraceContent = memo(
	({ className, children, ...props }: StackTraceContentProps) => (
		<CollapsibleContent className={cn("border-t bg-muted/30", className)} {...props}>
			{/* oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- a keyboard has to reach a scrolling region to scroll it. */}
			<div className="max-h-96 overflow-auto" tabIndex={0}>
				{children}
			</div>
		</CollapsibleContent>
	),
);

export type StackTraceFramesProps = ComponentProps<"div"> & {
	showInternalFrames?: boolean;
};

interface FilePathButtonProps {
	frame: StackFrame;
	onFilePathClick?: (filePath: string, lineNumber?: number, columnNumber?: number) => void;
}

const FilePathButton = memo(({ frame, onFilePathClick }: FilePathButtonProps) => {
	const handleClick = useCallback(() => {
		if (frame.filePath) {
			onFilePathClick?.(
				frame.filePath,
				frame.lineNumber ?? undefined,
				frame.columnNumber ?? undefined,
			);
		}
	}, [frame, onFilePathClick]);

	return (
		<button
			className={cn(
				"underline decoration-dotted hover:text-primary",
				onFilePathClick && "cursor-pointer",
			)}
			disabled={!onFilePathClick}
			onClick={handleClick}
			type="button"
		>
			{frame.filePath}
			{frame.lineNumber !== null && `:${frame.lineNumber}`}
			{frame.columnNumber !== null && `:${frame.columnNumber}`}
		</button>
	);
});

FilePathButton.displayName = "FilePathButton";

export const StackTraceFrames = memo(
	({ className, showInternalFrames = true, ...props }: StackTraceFramesProps) => {
		const { trace, onFilePathClick } = useStackTrace();

		const framesToShow = showInternalFrames
			? trace.frames
			: trace.frames.filter((f) => !f.isInternal);

		return (
			<div className={cn("space-y-1 p-3", className)} {...props}>
				{framesToShow.map((frame) => (
					<div
						className={cn(
							"whitespace-nowrap text-xs",
							frame.isInternal ? "text-muted-foreground/50" : "text-foreground/90",
						)}
						key={frame.raw}
					>
						<span className="text-muted-foreground">at </span>
						{frame.functionName && (
							<span className={frame.isInternal ? "" : "text-foreground"}>
								{frame.functionName}{" "}
							</span>
						)}
						{frame.filePath && (
							<>
								<span className="text-muted-foreground">(</span>
								<FilePathButton frame={frame} onFilePathClick={onFilePathClick} />
								<span className="text-muted-foreground">)</span>
							</>
						)}
						{!(frame.filePath || frame.functionName) && (
							<span>{frame.raw.replace(AT_PREFIX_REGEX, "")}</span>
						)}
					</div>
				))}
				{framesToShow.length === 0 && (
					<div className="text-muted-foreground text-xs">No stack frames</div>
				)}
			</div>
		);
	},
);

StackTrace.displayName = "StackTrace";
StackTraceHeader.displayName = "StackTraceHeader";
StackTraceError.displayName = "StackTraceError";
StackTraceErrorType.displayName = "StackTraceErrorType";
StackTraceErrorMessage.displayName = "StackTraceErrorMessage";
StackTraceExpandButton.displayName = "StackTraceExpandButton";
StackTraceContent.displayName = "StackTraceContent";
StackTraceFrames.displayName = "StackTraceFrames";
