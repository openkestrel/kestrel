"use client";

import { cjk } from "@streamdown/cjk";
import { code } from "@streamdown/code";
import type { ComponentProps } from "react";
import { memo } from "react";
import { Streamdown } from "streamdown";
import { cn } from "#/lib/utils";

export type MessageResponseProps = ComponentProps<typeof Streamdown>;

const streamdownPlugins = { cjk, code };

// A Transcript keeps a code block's copy and leaves every other control out of a message.
const streamdownControls = { code: { download: false }, table: false };

// Streamdown's own strong is a styled span, which assistive technology reads as plain text, and
// a task list's checkbox arrives with no name.
const streamdownComponents: MessageResponseProps["components"] = {
	input: ({ node: _node, ...props }) => (
		<input aria-label={props.type === "checkbox" ? "Task" : undefined} {...props} />
	),
	strong: ({ node: _node, className, ...props }) => (
		<strong className={cn("font-semibold", className)} data-streamdown="strong" {...props} />
	),
};

// GitHub's default themes paint some tokens below 4.5:1 on the page's background.
const streamdownThemes: MessageResponseProps["shikiTheme"] = [
	"github-light-high-contrast",
	"github-dark-high-contrast",
];

// A code block scrolls and Streamdown gives it no tab stop; highlighting replaces it after mount.
function scrollRegionsFocusable(root: HTMLElement | null): (() => void) | undefined {
	if (!root) return undefined;
	const mark = () => {
		for (const region of root.querySelectorAll(
			'[data-streamdown="code-block-body"]:not([tabindex])',
		)) {
			region.setAttribute("tabindex", "0");
		}
	};
	mark();
	const observer = new MutationObserver(mark);
	observer.observe(root, { childList: true, subtree: true });
	return () => observer.disconnect();
}

export const MessageResponse = memo(
	({ className, ...props }: MessageResponseProps) => (
		<div ref={scrollRegionsFocusable} className="contents">
			<Streamdown
				className={cn("size-full [&>*:first-child]:mt-0 [&>*:last-child]:mb-0", className)}
				components={streamdownComponents}
				controls={streamdownControls}
				plugins={streamdownPlugins}
				shikiTheme={streamdownThemes}
				{...props}
			/>
		</div>
	),
	(prevProps, nextProps) =>
		prevProps.children === nextProps.children && nextProps.isAnimating === prevProps.isAnimating,
);

MessageResponse.displayName = "MessageResponse";
