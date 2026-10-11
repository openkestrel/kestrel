import { type Provider, PROVIDERS } from "#/assets/provider-marks";
import type { ChangingOption, Session, SessionOption, SessionOptionValue } from "./generated";

export const HARNESS_DEFAULT = "the harness's default";

export type OfferedGroup = {
	group: string | null;
	name: string | null;
	values: SessionOptionValue[];
};

export function modelOption(options: SessionOption[]): SessionOption | undefined {
	return options.find((option) => option.category === "model" && option.kind === "select");
}

export function offeredGroups(option: SessionOption): OfferedGroup[] {
	return [{ group: null, name: null, values: option.values }, ...option.groups].filter(
		(group) => group.values.length > 0,
	);
}

export function offeredName(option: SessionOption, value: string): string {
	return (
		offeredGroups(option)
			.flatMap((group) => group.values)
			.find((offered) => offered.value === value)?.name ?? value
	);
}

// cmdk's own score is fuzzy across the whole id, so "opus" would keep every Anthropic model.
export function offeredMatch(value: string, search: string, keywords: string[] = []): number {
	const offered = [value, ...keywords].join(" ").toLowerCase();
	const terms = search.toLowerCase().split(/\s+/).filter(Boolean);
	return terms.every((term) => offered.includes(term)) ? 1 : 0;
}

const FAMILIES: [RegExp, Provider][] = [
	[/^claude/, "anthropic"],
	[/^(gpt|codex)/, "openai"],
	[/^gemini/, "google"],
];

function bundled(provider: string | null | undefined): Provider | undefined {
	const name = provider?.toLowerCase();
	return PROVIDERS.find((known) => known === name);
}

// Only ever decorates the model's own text, so a miss costs a mark and nothing else.
export function providerOf(value: string, group?: string | null): Provider | undefined {
	const qualified = value.includes("/") ? value.split("/")[0] : undefined;
	return (
		bundled(group) ??
		bundled(qualified) ??
		(qualified === undefined
			? FAMILIES.find(([family]) => family.test(value.toLowerCase()))?.[1]
			: undefined)
	);
}

export function changingModel(
	session: Pick<Session, "changing_options"> | undefined,
): ChangingOption | undefined {
	return session?.changing_options.find((change) => change.category === "model");
}

export function overrideLine(model: string, fallback: string): string {
	const id = model.trim();
	return id === ""
		? `Uses ${fallback}.`
		: `Overrides ${fallback} with ${id}, taken at its word until a Session reports what the harness offers.`;
}
