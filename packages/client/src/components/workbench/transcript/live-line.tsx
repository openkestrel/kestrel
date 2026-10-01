import type { TranscriptSessionState } from "#/operator/generated";
import { toolState } from "#/operator/transcript-view";

export function LiveLine({ state }: { state: TranscriptSessionState | undefined }) {
	if (!state || state.session_id === null) return null;

	const running = state.tools.filter((tool) => toolState(tool.status) === "running");
	const quiet = running.length === 0 && !state.message_buffering && !state.thought_buffering;
	if (quiet) return null;

	return (
		<output
			data-live
			className="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground text-xs"
		>
			{running.map((tool) => (
				<span key={tool.call_id} className="flex items-center gap-1">
					<span aria-hidden className="size-1.5 rounded-full bg-current" />
					{tool.title} · {tool.status}
				</span>
			))}
			{state.thought_buffering && <span>thinking…</span>}
			{state.message_buffering && <span>receiving a message…</span>}
		</output>
	);
}
