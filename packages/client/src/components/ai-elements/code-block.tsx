"use client";

import { CheckIcon, CopyIcon } from "lucide-react";
import type { ComponentProps, HTMLAttributes } from "react";
import {
	createContext,
	memo,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import type {
	BundledLanguage,
	BundledTheme,
	HighlighterGeneric,
	SpecialLanguage,
	ThemedToken,
} from "shiki";

// Tool output has no language to highlight, so plain text is a language here.
export type CodeLanguage = BundledLanguage | SpecialLanguage;
import { createHighlighter } from "shiki";
import { Button } from "#/components/ui/button.tsx";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "#/components/ui/select.tsx";
import { cn } from "#/lib/utils.ts";

// Shiki uses bitflags for font styles: 1=italic, 2=bold, 4=underline
// oxlint-disable-next-line eslint(no-bitwise)
const isItalic = (fontStyle: number | undefined) => fontStyle && fontStyle & 1;
// oxlint-disable-next-line eslint(no-bitwise)
const isBold = (fontStyle: number | undefined) => fontStyle && fontStyle & 2;
const isUnderline = (fontStyle: number | undefined) =>
	// oxlint-disable-next-line eslint(no-bitwise)
	fontStyle && fontStyle & 4;

interface KeyedToken {
	token: ThemedToken;
	key: string;
}
interface KeyedLine {
	tokens: KeyedToken[];
	key: string;
}

const addKeysToTokens = (lines: ThemedToken[][]): KeyedLine[] =>
	lines.map((line, lineIdx) => ({
		key: `line-${lineIdx}`,
		tokens: line.map((token, tokenIdx) => ({
			key: `line-${lineIdx}-${tokenIdx}`,
			token,
		})),
	}));

const TokenSpan = ({ token }: { token: ThemedToken }) => (
	<span
		className={cn(
			"dark:!bg-[var(--shiki-dark-bg)] dark:!text-[var(--shiki-dark)]",
			isItalic(token.fontStyle) && "italic",
			isBold(token.fontStyle) && "font-bold",
			isUnderline(token.fontStyle) && "underline",
		)}
		// oxlint-disable-next-line shadcn/no-inline-styles -- shiki's token colours are data.
		style={{ backgroundColor: token.bgColor, color: token.color, ...token.htmlStyle }}
	>
		{token.content}
	</span>
);

const LINE_NUMBER_CLASSES = cn(
	"block",
	"before:content-[counter(line)]",
	"before:inline-block",
	"before:[counter-increment:line]",
	"before:w-8",
	"before:mr-4",
	"before:text-right",
	"before:text-muted-foreground/50",
	"before:font-mono",
	"before:select-none",
);

const LineSpan = ({
	keyedLine,
	showLineNumbers,
}: {
	keyedLine: KeyedLine;
	showLineNumbers: boolean;
}) => (
	<span className={showLineNumbers ? LINE_NUMBER_CLASSES : "block"}>
		{keyedLine.tokens.length === 0
			? "\n"
			: keyedLine.tokens.map(({ token, key }) => <TokenSpan key={key} token={token} />)}
	</span>
);

type CodeBlockProps = HTMLAttributes<HTMLDivElement> & {
	code: string;
	language: CodeLanguage;
	showLineNumbers?: boolean;
};

interface TokenizedCode {
	tokens: ThemedToken[][];
	fg: string;
	bg: string;
}

interface CodeBlockContextType {
	code: string;
}

const CodeBlockContext = createContext<CodeBlockContextType>({
	code: "",
});

const highlighterCache = new Map<
	string,
	Promise<HighlighterGeneric<BundledLanguage, BundledTheme>>
>();

const tokensCache = new Map<string, TokenizedCode>();

const subscribers = new Map<string, Set<(result: TokenizedCode) => void>>();

const getTokensCacheKey = (code: string, language: CodeLanguage) => {
	const start = code.slice(0, 100);
	const end = code.length > 100 ? code.slice(-100) : "";
	return `${language}:${code.length}:${start}:${end}`;
};

const getHighlighter = (
	language: CodeLanguage,
): Promise<HighlighterGeneric<BundledLanguage, BundledTheme>> => {
	const cached = highlighterCache.get(language);
	if (cached) {
		return cached;
	}

	const highlighterPromise = createHighlighter({
		langs: [language],
		themes: ["github-light", "github-dark"],
	});

	highlighterCache.set(language, highlighterPromise);
	return highlighterPromise;
};

const createRawTokens = (code: string): TokenizedCode => ({
	bg: "transparent",
	fg: "inherit",
	tokens: code.split("\n").map((line) =>
		line === ""
			? []
			: [
					{
						color: "inherit",
						content: line,
						offset: 0,
					},
				],
	),
});

export const highlightCode = (
	code: string,
	language: CodeLanguage,
	// oxlint-disable-next-line eslint-plugin-promise(prefer-await-to-callbacks)
	callback?: (result: TokenizedCode) => void,
): TokenizedCode | null => {
	const tokensCacheKey = getTokensCacheKey(code, language);

	const cached = tokensCache.get(tokensCacheKey);
	if (cached) {
		return cached;
	}

	if (callback) {
		if (!subscribers.has(tokensCacheKey)) {
			subscribers.set(tokensCacheKey, new Set());
		}
		subscribers.get(tokensCacheKey)?.add(callback);
	}

	getHighlighter(language)
		// oxlint-disable-next-line eslint-plugin-promise(prefer-await-to-then)
		.then((highlighter) => {
			const availableLangs = highlighter.getLoadedLanguages();
			const langToUse = availableLangs.includes(language) ? language : "text";

			const result = highlighter.codeToTokens(code, {
				lang: langToUse,
				themes: {
					dark: "github-dark",
					light: "github-light",
				},
			});

			const tokenized: TokenizedCode = {
				bg: result.bg ?? "transparent",
				fg: result.fg ?? "inherit",
				tokens: result.tokens,
			};

			tokensCache.set(tokensCacheKey, tokenized);

			const subs = subscribers.get(tokensCacheKey);
			if (subs) {
				for (const sub of subs) {
					sub(tokenized);
				}
				subscribers.delete(tokensCacheKey);
			}
		})
		// oxlint-disable-next-line eslint-plugin-promise(prefer-await-to-then), eslint-plugin-promise(prefer-await-to-callbacks)
		.catch((error) => {
			console.error("Failed to highlight code:", error);
			subscribers.delete(tokensCacheKey);
		});

	return null;
};

const CodeBlockBody = memo(
	({
		tokenized,
		showLineNumbers,
		className,
	}: {
		tokenized: TokenizedCode;
		showLineNumbers: boolean;
		className?: string;
	}) => {
		const keyedLines = useMemo(() => addKeysToTokens(tokenized.tokens), [tokenized.tokens]);

		return (
			<pre
				className={cn(
					"dark:!bg-[var(--shiki-dark-bg)] dark:!text-[var(--shiki-dark)] m-0 p-4 text-sm",
					className,
				)}
				// oxlint-disable-next-line shadcn/no-inline-styles -- shiki's theme colours are data.
				style={{ backgroundColor: tokenized.bg, color: tokenized.fg }}
			>
				<code
					className={cn(
						"font-mono text-sm",
						showLineNumbers && "[counter-increment:line_0] [counter-reset:line]",
					)}
				>
					{keyedLines.map((keyedLine) => (
						<LineSpan key={keyedLine.key} keyedLine={keyedLine} showLineNumbers={showLineNumbers} />
					))}
				</code>
			</pre>
		);
	},
	(prevProps, nextProps) =>
		prevProps.tokenized === nextProps.tokenized &&
		prevProps.showLineNumbers === nextProps.showLineNumbers &&
		prevProps.className === nextProps.className,
);

CodeBlockBody.displayName = "CodeBlockBody";

export const CodeBlockContainer = ({
	className,
	language,
	...props
}: HTMLAttributes<HTMLDivElement> & { language: string }) => (
	<div
		className={cn(
			"group relative w-full overflow-hidden rounded-md border bg-background text-foreground [contain-intrinsic-size:auto_200px] [content-visibility:auto]",
			className,
		)}
		data-language={language}
		{...props}
	/>
);

export const CodeBlockHeader = ({
	children,
	className,
	...props
}: HTMLAttributes<HTMLDivElement>) => (
	<div
		className={cn(
			"flex items-center justify-between border-b bg-muted/80 px-3 py-2 text-muted-foreground text-xs",
			className,
		)}
		{...props}
	>
		{children}
	</div>
);

export const CodeBlockTitle = ({
	children,
	className,
	...props
}: HTMLAttributes<HTMLDivElement>) => (
	<div className={cn("flex items-center gap-2", className)} {...props}>
		{children}
	</div>
);

export const CodeBlockFilename = ({
	children,
	className,
	...props
}: HTMLAttributes<HTMLSpanElement>) => (
	<span className={cn("font-mono", className)} {...props}>
		{children}
	</span>
);

export const CodeBlockActions = ({
	children,
	className,
	...props
}: HTMLAttributes<HTMLDivElement>) => (
	<div className={cn("-my-1 -mr-1 flex items-center gap-2", className)} {...props}>
		{children}
	</div>
);

export const CodeBlockContent = ({
	code,
	language,
	showLineNumbers = false,
	className,
	...props
}: HTMLAttributes<HTMLDivElement> & {
	code: string;
	language: CodeLanguage;
	showLineNumbers?: boolean;
}) => {
	const rawTokens = useMemo(() => createRawTokens(code), [code]);

	const syncTokens = useMemo(
		() => highlightCode(code, language) ?? rawTokens,
		[code, language, rawTokens],
	);

	// Tokens are kept with what they highlight, so a stale result is never shown for new code.
	const [highlighted, setHighlighted] = useState<{
		code: string;
		language: CodeLanguage;
		tokens: TokenizedCode;
	} | null>(null);

	useEffect(() => {
		let cancelled = false;

		highlightCode(code, language, (tokens) => {
			if (!cancelled) {
				setHighlighted({ code, language, tokens });
			}
		});

		return () => {
			cancelled = true;
		};
	}, [code, language]);

	const tokenized =
		highlighted?.code === code && highlighted.language === language
			? highlighted.tokens
			: syncTokens;

	return (
		// oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- a scrollable region must be focusable for keyboard scrolling.
		<div className={cn("relative overflow-auto", className)} tabIndex={0} {...props}>
			<CodeBlockBody showLineNumbers={showLineNumbers} tokenized={tokenized} />
		</div>
	);
};

export const CodeBlock = ({
	code,
	language,
	showLineNumbers = false,
	className,
	children,
	...props
}: CodeBlockProps) => {
	const contextValue = useMemo(() => ({ code }), [code]);

	return (
		<CodeBlockContext.Provider value={contextValue}>
			<CodeBlockContainer className={className} language={language} {...props}>
				{children}
				<CodeBlockContent code={code} language={language} showLineNumbers={showLineNumbers} />
			</CodeBlockContainer>
		</CodeBlockContext.Provider>
	);
};

export type CodeBlockCopyButtonProps = ComponentProps<typeof Button> & {
	onCopy?: () => void;
	onError?: (error: Error) => void;
	timeout?: number;
};

export const CodeBlockCopyButton = ({
	onCopy,
	onError,
	timeout = 2000,
	children,
	className,
	...props
}: CodeBlockCopyButtonProps) => {
	const [isCopied, setIsCopied] = useState(false);
	const timeoutRef = useRef<number>(0);
	const { code } = useContext(CodeBlockContext);

	const copyToClipboard = useCallback(async () => {
		try {
			if (!isCopied) {
				await navigator.clipboard.writeText(code);
				setIsCopied(true);
				onCopy?.();
				timeoutRef.current = window.setTimeout(() => setIsCopied(false), timeout);
			}
		} catch (error) {
			onError?.(error instanceof Error ? error : new Error(String(error)));
		}
	}, [code, onCopy, onError, timeout, isCopied]);

	useEffect(
		() => () => {
			window.clearTimeout(timeoutRef.current);
		},
		[],
	);

	const Icon = isCopied ? CheckIcon : CopyIcon;

	return (
		<Button
			className={cn("shrink-0", className)}
			onClick={copyToClipboard}
			size="icon"
			variant="ghost"
			{...props}
		>
			{children ?? <Icon size={14} />}
		</Button>
	);
};

export type CodeBlockLanguageSelectorProps = ComponentProps<typeof Select>;

export const CodeBlockLanguageSelector = (props: CodeBlockLanguageSelectorProps) => (
	<Select {...props} />
);

export type CodeBlockLanguageSelectorTriggerProps = ComponentProps<typeof SelectTrigger>;

export const CodeBlockLanguageSelectorTrigger = ({
	className,
	...props
}: CodeBlockLanguageSelectorTriggerProps) => (
	<SelectTrigger
		className={cn("h-7 border-none bg-transparent px-2 text-xs shadow-none", className)}
		size="sm"
		{...props}
	/>
);

export type CodeBlockLanguageSelectorValueProps = ComponentProps<typeof SelectValue>;

export const CodeBlockLanguageSelectorValue = (props: CodeBlockLanguageSelectorValueProps) => (
	<SelectValue {...props} />
);

export type CodeBlockLanguageSelectorContentProps = ComponentProps<typeof SelectContent>;

export const CodeBlockLanguageSelectorContent = ({
	align = "end",
	...props
}: CodeBlockLanguageSelectorContentProps) => <SelectContent align={align} {...props} />;

export type CodeBlockLanguageSelectorItemProps = ComponentProps<typeof SelectItem>;

export const CodeBlockLanguageSelectorItem = (props: CodeBlockLanguageSelectorItemProps) => (
	<SelectItem {...props} />
);
