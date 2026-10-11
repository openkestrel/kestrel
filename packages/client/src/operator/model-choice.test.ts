import { describe, expect, it } from "vitest";
import type { SessionOption } from "./generated";
import {
	changingModel,
	modelOption,
	offeredGroups,
	offeredMatch,
	offeredName,
	overrideLine,
	providerOf,
} from "./model-choice";

function option(overrides: Partial<SessionOption> = {}): SessionOption {
	return {
		id: "model",
		name: "Model",
		description: null,
		category: "model",
		kind: "select",
		current: "mini",
		values: [
			{ value: "mini", name: "Mini", description: null },
			{ value: "max", name: "Max", description: "slower" },
		],
		groups: [],
		warns_cache: true,
		...overrides,
	};
}

describe("modelOption", () => {
	it("is the select the harness categorised as its model", () => {
		const mode = option({ id: "mode", category: "mode" });
		expect(modelOption([mode, option()])?.id).toBe("model");
	});

	it("is absent when the harness offers no model", () => {
		expect(modelOption([option({ category: "mode" })])).toBeUndefined();
		expect(modelOption([option({ kind: "boolean", current: true })])).toBeUndefined();
	});
});

describe("offeredGroups", () => {
	it("keeps an ungrouped select as one unnamed group", () => {
		expect(offeredGroups(option())).toEqual([{ group: null, name: null, values: option().values }]);
	});

	it("keeps the harness's own groups in its order and drops an empty one", () => {
		const grouped = option({
			values: [],
			groups: [
				{
					group: "openai",
					name: "OpenAI",
					values: [{ value: "gpt", name: "GPT", description: null }],
				},
				{ group: "empty", name: "Empty", values: [] },
				{
					group: "anthropic",
					name: "Anthropic",
					values: [{ value: "claude", name: "Claude", description: null }],
				},
			],
		});
		expect(offeredGroups(grouped).map((group) => group.name)).toEqual(["OpenAI", "Anthropic"]);
	});
});

describe("offeredName", () => {
	it("is the offered name of a value, grouped or not", () => {
		const grouped = option({
			values: [],
			groups: [
				{
					group: "g",
					name: "G",
					values: [{ value: "claude", name: "Claude", description: null }],
				},
			],
		});
		expect(offeredName(option(), "max")).toBe("Max");
		expect(offeredName(grouped, "claude")).toBe("Claude");
	});

	it("is the id itself for a value the harness no longer offers", () => {
		expect(offeredName(option(), "retired")).toBe("retired");
	});
});

describe("offeredMatch", () => {
	it("keeps a model whose id, name or group holds every word searched", () => {
		const keywords = ["Claude Opus 4.5", "Anthropic"];
		expect(offeredMatch("anthropic/claude-opus-4-5", "opus", keywords)).toBe(1);
		expect(offeredMatch("anthropic/claude-opus-4-5", "ANTHROPIC  4.5", keywords)).toBe(1);
		expect(offeredMatch("anthropic/claude-opus-4-5", "", keywords)).toBe(1);
	});

	it("drops one that only holds the letters in order", () => {
		expect(
			offeredMatch("anthropic/claude-sonnet-4-5", "opus", ["Claude Sonnet 4.5", "Anthropic"]),
		).toBe(0);
	});
});

describe("providerOf", () => {
	it("reads a bundled provider from the group the harness put the value in", () => {
		expect(providerOf("sonnet-4-5", "anthropic")).toBe("anthropic");
		expect(providerOf("sonnet-4-5", "Anthropic")).toBe("anthropic");
	});

	it("reads it from a provider-qualified id", () => {
		expect(providerOf("openai/gpt-5.4")).toBe("openai");
		expect(providerOf("opencode/big-pickle")).toBe("opencode");
	});

	it("reads it from a vendor's own model family", () => {
		expect(providerOf("claude-opus-4-5")).toBe("anthropic");
		expect(providerOf("gpt-5.4-mini")).toBe("openai");
		expect(providerOf("gemini-3-pro")).toBe("google");
	});

	it("names none for a provider with no bundled mark", () => {
		expect(providerOf("scripted-mini")).toBeUndefined();
		expect(providerOf("mistral/large", "mistral")).toBeUndefined();
	});
});

describe("changingModel", () => {
	it("is the model change the harness has not answered yet", () => {
		const change = { option: "model", category: "model", value: "max", participant: "jack" };
		const mode = { option: "mode", category: "mode", value: "plan", participant: "jack" };
		expect(changingModel({ changing_options: [mode, change] })).toEqual(change);
		expect(changingModel({ changing_options: [mode] })).toBeUndefined();
		expect(changingModel(undefined)).toBeUndefined();
	});
});

describe("overrideLine", () => {
	it("says the default applies when no model id is entered", () => {
		expect(overrideLine("  ", "the harness's default")).toBe("Uses the harness's default.");
	});

	it("says an entered id is taken at its word", () => {
		expect(overrideLine(" gpt-5.4 ", "the harness's default")).toBe(
			"Overrides the harness's default with gpt-5.4, taken at its word until a Session reports what the harness offers.",
		);
	});
});
