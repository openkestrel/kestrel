import { describe, expect, it } from "vitest";
import { agentDeclaration, agentLine, draftOfAgent, harnessChoices } from "./agents";
import type { Agent, HarnessCatalogueEntry } from "./generated";

const builder: Agent = {
	id: "00000000-0000-0000-0000-000000000001",
	name: "builder",
	harness: "claude",
	model: "opus",
	mode: "plan",
	thought_level: null,
};

function entry(
	name: string,
	state: NonNullable<HarnessCatalogueEntry["availability"]>["state"],
): HarnessCatalogueEntry {
	return {
		name,
		command: `${name} acp`,
		sign_in_methods: [],
		availability: { state, image: "kestrel-env:main", identity: null, diagnostic: null },
	};
}

describe("agentDeclaration", () => {
	it("sends back every default an existing Agent declares when only one field changed", () => {
		expect(agentDeclaration({ ...draftOfAgent(builder), model: "sonnet" })).toEqual({
			name: "builder",
			harness: "claude",
			model: "sonnet",
			mode: "plan",
			thought_level: null,
		});
	});

	it("names no model, mode or thought level for an empty field", () => {
		expect(
			agentDeclaration({
				name: " builder ",
				harness: "codex",
				model: " ",
				mode: "",
				thought_level: "",
			}),
		).toEqual({ name: "builder", harness: "codex", model: null, mode: null, thought_level: null });
	});
});

describe("harnessChoices", () => {
	it("offers the catalogue's harnesses and withholds one the image is known to lack", () => {
		expect(
			harnessChoices(
				[
					entry("opencode", "available"),
					entry("claude", "not_carried"),
					entry("codex", "unchecked"),
				],
				"",
			),
		).toEqual([
			{ name: "opencode", command: "opencode acp", selectable: true, note: null },
			{ name: "claude", command: "claude acp", selectable: false, note: "not in the image" },
			{ name: "codex", command: "codex acp", selectable: true, note: null },
		]);
	});

	it("claims nothing about a harness whose image could not be inspected", () => {
		expect(harnessChoices([entry("claude", "unavailable")], "")).toEqual([
			{ name: "claude", command: "claude acp", selectable: true, note: "image not inspected" },
		]);
	});

	it("keeps the harness an Agent already declares, whatever the catalogue says", () => {
		expect(harnessChoices([entry("claude", "not_carried")], "claude")[0].selectable).toBe(true);
		expect(harnessChoices([entry("claude", "available")], "house")).toContainEqual({
			name: "house",
			command: null,
			selectable: true,
			note: "not in the catalogue",
		});
	});

	it("keeps an entered harness while the catalogue is unread, saying nothing about it", () => {
		expect(harnessChoices(undefined, "claude")).toEqual([
			{ name: "claude", command: null, selectable: true, note: null },
		]);
	});
});

describe("agentLine", () => {
	it("names the harness and each default the Agent declares", () => {
		expect(agentLine(builder)).toBe("claude · model opus · mode plan");
		expect(agentLine({ ...builder, model: null, mode: null })).toBe(
			"claude · model the harness's default",
		);
	});
});
