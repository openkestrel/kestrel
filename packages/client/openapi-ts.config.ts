import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { defineConfig } from "@hey-api/openapi-ts";

type Json = { [key: string]: unknown };

// The document is authored as one fragment per area; `kestrel_operator_types::document` merges
// them the same way for the Rust side.
function merge(whole: Json, fragment: Json, at = ""): Json {
	for (const [key, value] of Object.entries(fragment)) {
		const held = whole[key];
		if (held === undefined) whole[key] = value;
		else if (isObject(held) && isObject(value)) merge(held, value, `${at}/${key}`);
		else throw new Error(`the operator document defines ${at}/${key} twice`);
	}
	return whole;
}

function isObject(value: unknown): value is Json {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

const fragments = "../../openapi/operator";
const document = readdirSync(fragments)
	.filter((file) => file.endsWith(".json"))
	.toSorted()
	.reduce<Json>(
		(whole, file) => merge(whole, JSON.parse(readFileSync(join(fragments, file), "utf8"))),
		{},
	);

// Types only: the transport owns every request, so refusals and streams behave as the CLI's do (ADR-0036).
export default defineConfig({
	input: document,
	output: { path: "src/operator/generated", postProcess: [] },
	plugins: ["@hey-api/typescript"],
});
