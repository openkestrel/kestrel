import { queryOptions } from "@tanstack/react-query";
import type { Agent, AgentDeclaration, HarnessCatalogueEntry } from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export const harnessesQuery = queryOptions({
	queryKey: ["harnesses"],
	queryFn: ({ signal }) =>
		operator.read<HarnessCatalogueEntry[]>(operatorPath("harnesses"), { signal }),
});

export function declareAgent(organization: string, declaration: AgentDeclaration): Promise<Agent> {
	return operator.write<Agent>(
		"POST",
		operatorPath("organizations", organization, "agents"),
		declaration,
	);
}

export type AgentDraft = {
	name: string;
	harness: string;
	model: string;
	mode: string;
	thought_level: string;
};

export const A_NEW_AGENT: AgentDraft = {
	name: "",
	harness: "",
	model: "",
	mode: "",
	thought_level: "",
};

export function draftOfAgent(agent: Agent): AgentDraft {
	return {
		name: agent.name,
		harness: agent.harness,
		model: agent.model ?? "",
		mode: agent.mode ?? "",
		thought_level: agent.thought_level ?? "",
	};
}

function named(value: string): string | null {
	return value.trim() === "" ? null : value.trim();
}

// A declaration is the Agent's whole desired state, so every field is sent and an empty one names none.
export function agentDeclaration(draft: AgentDraft): AgentDeclaration {
	return {
		name: draft.name.trim(),
		harness: draft.harness,
		model: named(draft.model),
		mode: named(draft.mode),
		thought_level: named(draft.thought_level),
	};
}

export type HarnessChoice = {
	name: string;
	command: string | null;
	selectable: boolean;
	note: string | null;
};

const NOTES: Record<NonNullable<HarnessCatalogueEntry["availability"]>["state"], string | null> = {
	available: null,
	unchecked: null,
	not_carried: "not in the image",
	unavailable: "image not inspected",
};

// A failed inspection claims nothing about the harness, so only one the image is known to lack is withheld.
export function harnessChoices(
	catalogue: HarnessCatalogueEntry[] | undefined,
	declared: string,
): HarnessChoice[] {
	const choices = (catalogue ?? []).map(({ name, command, availability }) => ({
		name,
		command,
		selectable: availability?.state !== "not_carried" || name === declared,
		note: availability ? NOTES[availability.state] : null,
	}));
	if (declared === "" || choices.some((choice) => choice.name === declared)) return choices;
	return [
		...choices,
		{
			name: declared,
			command: null,
			selectable: true,
			note: catalogue ? "not in the catalogue" : null,
		},
	];
}

export function agentLine(agent: Agent): string {
	const defaults = [
		`model ${agent.model ?? "the harness's default"}`,
		agent.mode && `mode ${agent.mode}`,
		agent.thought_level && `thought level ${agent.thought_level}`,
	];
	return [agent.harness, ...defaults].filter(Boolean).join(" · ");
}
