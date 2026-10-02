import { describe, expect, it } from "vitest";
import {
	amendable,
	cacheWarning,
	heldAge,
	mayWriteOptions,
	modeOption,
	modeStep,
	nameToChangeOptions,
	nextMode,
	optionChange,
	optionValues,
	partialReport,
	postLabel,
	wasAmended,
} from "./composer";
import type { HeldMessage, SessionOption, Usage } from "./generated";

function option(overrides: Partial<SessionOption> = {}): SessionOption {
	return {
		id: "model",
		name: "Model",
		description: null,
		category: "model",
		kind: "select",
		current: "mini",
		values: [
			{ value: "mini", name: "mini", description: null },
			{ value: "max", name: "max", description: null },
		],
		groups: [],
		warns_cache: true,
		...overrides,
	};
}

function held(overrides: Partial<HeldMessage> = {}): HeldMessage {
	return {
		id: 1,
		participant: "jack",
		message: "wait for me",
		posted_at: "2026-09-30T10:00:00Z",
		edited_at: null,
		...overrides,
	};
}

const usage: Usage = { context_used: 1_200, context_size: 200_000, cost: null };

describe("the composer's label", () => {
	it("adds to the next turn while the Session works, and posts otherwise", () => {
		expect(postLabel("working")).toBe("Add to next turn");
		expect(postLabel("waiting")).toBe("Post");
		expect(postLabel(undefined)).toBe("Post");
	});
});

describe("who may write options", () => {
	it("is nobody while a Turn is working", () => {
		expect(mayWriteOptions("working")).toBe(false);
		expect(mayWriteOptions("waiting")).toBe(true);
	});
});

describe("a held message", () => {
	it("is amendable by its own author and no one else", () => {
		expect(amendable(held(), "jack")).toBe(true);
		expect(amendable(held(), "jill")).toBe(false);
		expect(amendable(held(), null)).toBe(false);
	});

	it("says when it was edited", () => {
		expect(wasAmended(held())).toBe(false);
		expect(wasAmended(held({ edited_at: "2026-09-30T10:01:00Z" }))).toBe(true);
	});

	it("reads its age from the moment it was posted", () => {
		const now = Date.parse("2026-09-30T10:00:00Z");
		expect(heldAge("2026-09-30T09:59:55Z", now)).toBe("just now");
		expect(heldAge("2026-09-30T09:58:00Z", now)).toBe("2 minutes ago");
		expect(heldAge("2026-09-30T07:00:00Z", now)).toBe("3 hours ago");
	});
});

describe("an option", () => {
	it("lists its values, grouped ones included", () => {
		expect(
			optionValues(
				option({
					values: [{ value: "mini", name: "mini", description: null }],
					groups: [
						{
							group: "fast",
							name: "Fast",
							values: [{ value: "max", name: "max", description: null }],
						},
					],
				}),
			).map((value) => value.value),
		).toEqual(["mini", "max"]);
	});

	it("cycles to the next value and wraps", () => {
		expect(nextMode(option({ category: "mode", current: "mini" }), "mini")?.value).toBe("max");
		expect(nextMode(option({ category: "mode", current: "max" }), "max")?.value).toBe("mini");
		expect(nextMode(option({ category: "mode", values: [] }), "mini")).toBeUndefined();
	});

	it("is the mode the keyboard cycles", () => {
		expect(modeOption([option()])).toBeUndefined();
		expect(modeOption([option(), option({ id: "mode", category: "mode" })])?.id).toBe("mode");
	});

	it("warns about the cache with the context it last reported", () => {
		expect(cacheWarning(option(), usage)).toContain("1,200 tokens");
		expect(cacheWarning(option(), null)).not.toContain("tokens");
		expect(cacheWarning(option({ warns_cache: false }), usage)).toBeUndefined();
	});

	it("is named by its category when it has one kestrel knows, and by its id otherwise", () => {
		expect(optionChange(option({ category: "model" }), "max", "jack")).toEqual({
			participant: "jack",
			category: "model",
			value: "max",
		});
		expect(optionChange(option({ id: "verbose", category: "_scripted" }), "true", "jack")).toEqual({
			participant: "jack",
			option: "verbose",
			value: "true",
		});
	});
});

describe("Shift+Tab", () => {
	const mode = option({ id: "mode", category: "mode", current: "mini" });

	it("changes the mode to the next value for a named person between Turns", () => {
		expect(modeStep([mode], "waiting", "jack")).toEqual({ option: mode, value: "max" });
	});

	it("asks for a name, as the header does, before changing anything", () => {
		expect(modeStep([mode], "waiting", null)).toEqual({ say: nameToChangeOptions });
	});

	it("changes nothing during a working Turn or without a mode to cycle", () => {
		expect(modeStep([mode], "working", "jack")).toEqual({
			say: "The mode cannot change during a working turn",
		});
		expect(modeStep([option()], "waiting", "jack")).toEqual({
			say: "This harness offers no mode to cycle",
		});
	});
});

describe("send now's two writes", () => {
	it("says plainly when only the post landed", () => {
		expect(partialReport(true, false)).toBe(
			"The message was posted, but the Turn was not interrupted",
		);
		expect(partialReport(true, true)).toBeUndefined();
		expect(partialReport(false, false)).toBeUndefined();
	});
});
