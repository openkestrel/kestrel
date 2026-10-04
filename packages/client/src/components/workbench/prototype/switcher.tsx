// PROTOTYPE (#492): the floating bar that flips variant, typeface and theme.
import { ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import { useEffect } from "react";

export const FONTS = {
	inter: { label: "Inter", family: "'Inter Variable'" },
	geist: { label: "Geist", family: "'Geist Variable'" },
	plex: { label: "IBM Plex Sans", family: "'IBM Plex Sans Variable'" },
	mono: { label: "JetBrains Mono (today)", family: "'JetBrains Mono Variable'" },
} as const;

export type Font = keyof typeof FONTS;
export type Theme = "light" | "dark";

export function PrototypeSwitcher<Key extends string>({
	variants,
	current,
	name,
	font,
	theme,
	onChange,
}: {
	variants: readonly Key[];
	current: Key;
	name: string;
	font: Font;
	theme: Theme;
	onChange: (next: { variant?: Key; font?: Font; theme?: Theme }) => void;
}) {
	const step = (by: number) => {
		const index = variants.indexOf(current);
		onChange({ variant: variants[(index + by + variants.length) % variants.length] });
	};

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			const target = event.target as HTMLElement | null;
			if (target?.closest("input, textarea, [contenteditable], [role=dialog]")) return;
			if (event.key === "ArrowLeft") step(-1);
			if (event.key === "ArrowRight") step(1);
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	});

	if (import.meta.env.PROD) return null;

	return (
		<div
			data-prototype-switcher
			className="fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full bg-fuchsia-700 px-2 py-1 font-[system-ui] text-white text-xs shadow-lg"
		>
			<button type="button" aria-label="Previous variant" onClick={() => step(-1)} className="rounded-full p-1 hover:bg-white/20">
				<ChevronLeftIcon className="size-4" />
			</button>
			<span className="whitespace-nowrap px-1 font-semibold">
				{current} ({name})
			</span>
			<button type="button" aria-label="Next variant" onClick={() => step(1)} className="rounded-full p-1 hover:bg-white/20">
				<ChevronRightIcon className="size-4" />
			</button>
			<select
				aria-label="Typeface"
				value={font}
				onChange={(event) => onChange({ font: event.target.value as Font })}
				className="rounded-full bg-white/15 px-2 py-1"
			>
				{Object.entries(FONTS).map(([key, value]) => (
					<option key={key} value={key} className="text-black">
						{value.label}
					</option>
				))}
			</select>
			<button
				type="button"
				onClick={() => onChange({ theme: theme === "dark" ? "light" : "dark" })}
				className="rounded-full bg-white/15 px-2 py-1 hover:bg-white/25"
			>
				{theme}
			</button>
		</div>
	);
}
