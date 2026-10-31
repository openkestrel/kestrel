import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { bundle } from "./document";

// The Rust bundler reads the same fixtures, so the two cannot disagree unnoticed.
const fixture = (path: string) =>
	fileURLToPath(
		new URL(`../../../../crates/kestrel-operator-types/tests/document/${path}`, import.meta.url),
	);

describe("a split operator document", () => {
	it("bundles into one whose refs name its components", () => {
		const expected = JSON.parse(readFileSync(fixture("bundled.json"), "utf8"));
		expect(bundle(fixture("split/operator.json"))).toEqual(expected);
	});

	it("refuses a schema name two files define", () => {
		expect(() => bundle(fixture("collision/operator.json"))).toThrow(/Thing/);
	});

	it("refuses a file beside the root that no reference reaches", () => {
		expect(() => bundle(fixture("unreached/operator.json"))).toThrow(/stray\.json/);
	});
});
