import type {
	HeldMessage,
	OptionChange,
	Session,
	SessionOption,
	SessionOptionValue,
	Usage,
} from "./generated";

const CATEGORIES = new Set(["model", "mode", "thought_level"]);

export function optionChange(
	option: SessionOption,
	value: string,
	participant: string,
): OptionChange {
	return option.category && CATEGORIES.has(option.category)
		? { participant, category: option.category, value }
		: { participant, option: option.id, value };
}

export function postLabel(state: Session["state"] | undefined): string {
	return state === "working" ? "Add to next turn" : "Post";
}

// A working Turn is the harness's; an option write is refused, so the UI offers none.
export function mayWriteOptions(state: Session["state"] | undefined): boolean {
	return state !== "working";
}

export function amendable(held: HeldMessage, participant: string | null): boolean {
	return participant !== null && held.participant === participant;
}

export function wasAmended(held: HeldMessage): boolean {
	return held.edited_at !== null;
}

export function heldAge(postedAt: string, now: number = Date.now()): string {
	const seconds = Math.max(0, Math.round((now - Date.parse(postedAt)) / 1000));
	if (seconds < 45) return "just now";
	if (seconds < 90) return "a minute ago";
	const minutes = Math.round(seconds / 60);
	if (minutes < 60) return `${minutes} minutes ago`;
	const hours = Math.round(minutes / 60);
	if (hours < 24) return `${hours} hours ago`;
	return `${Math.round(hours / 24)} days ago`;
}

export function optionValues(option: SessionOption): SessionOptionValue[] {
	return [...option.values, ...option.groups.flatMap((group) => group.values)];
}

export function modeOption(options: SessionOption[]): SessionOption | undefined {
	return options.find((option) => option.category === "mode");
}

export function nextMode(option: SessionOption, current: string): SessionOptionValue | undefined {
	const values = optionValues(option);
	if (values.length === 0) return undefined;
	const at = values.findIndex((value) => value.value === current);
	return values[(at + 1 + values.length) % values.length];
}

export const nameToChangeOptions = "Declare your name in the composer to change options";

export type ModeStep = { option: SessionOption; value: string } | { say: string };

export function modeStep(
	options: SessionOption[],
	state: Session["state"] | undefined,
	participant: string | null,
): ModeStep {
	const option = modeOption(options);
	const next = option && nextMode(option, String(option.current));
	if (!option || !next) return { say: "This harness offers no mode to cycle" };
	if (!mayWriteOptions(state)) return { say: "The mode cannot change during a working turn" };
	if (participant === null) return { say: nameToChangeOptions };
	return { option, value: next.value };
}

export function cacheWarning(
	option: SessionOption,
	usage: Usage | null | undefined,
): string | undefined {
	if (!option.warns_cache) return undefined;
	const tokens = usage?.context_used;
	return tokens === undefined
		? "Changing this makes the next turn re-read the context without the prompt cache"
		: `Changing this makes the next turn re-read the context without the prompt cache: ${tokens.toLocaleString()} tokens`;
}

// Send now is two writes. Only one of them landing is said plainly, never as an atomic success.
export function partialReport(posted: boolean, interrupted: boolean): string | undefined {
	if (posted && !interrupted) {
		return "The message was posted, but the Turn was not interrupted";
	}
	return undefined;
}
