import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect } from "react";

export function SetupSwitcher({
	variant,
	name,
	theme,
	onSearch,
}: {
	variant: "A" | "B" | "C";
	name: string;
	theme: "light" | "dark";
	onSearch: (next: { variant?: "A" | "B" | "C"; theme?: "light" | "dark" }) => void;
}) {
	function cycle(offset: number) {
		const variants = ["A", "B", "C"] as const;
		onSearch({ variant: variants[(variants.indexOf(variant) + offset + 3) % 3] });
	}
	useEffect(() => {
		function onKey(event: KeyboardEvent) {
			if (
				(event.target instanceof HTMLElement ? event.target : null)?.closest(
					"input, textarea, select, [contenteditable], [role=dialog]",
				)
			)
				return;
			if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
				event.preventDefault();
				cycle(event.key === "ArrowLeft" ? -1 : 1);
			}
		}
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	});
	if (import.meta.env.PROD) return null;
	return (
		<div className="fixed bottom-3 left-1/2 z-50 flex w-max max-w-full -translate-x-1/2 items-center gap-2 rounded-full bg-primary px-3 py-2 text-xs text-primary-foreground shadow-lg">
			<button type="button" aria-label="Previous variant" onClick={() => cycle(-1)}>
				<ChevronLeft className="size-4" />
			</button>
			<span>
				{variant} · {name}
			</span>
			<button type="button" aria-label="Next variant" onClick={() => cycle(1)}>
				<ChevronRight className="size-4" />
			</button>
			<button
				type="button"
				className="border-l border-white/30 pl-2"
				onClick={() => onSearch({ theme: theme === "light" ? "dark" : "light" })}
			>
				{theme}
			</button>
		</div>
	);
}
