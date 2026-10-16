import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { document, isObject, type Json } from "./operator-document";

// Each streaming union names its variants' SSE events with `x-sse-event`; the Rust side derives
// `event_name` and `from_sse` from the same extension in `kestrel-operator-types/build.rs`.
const reference = "#/components/schemas/";
const components = document["components"];
const schemas =
	isObject(components) && isObject(components["schemas"]) ? components["schemas"] : {};

let source = "// Generated from openapi/operator/ by operator-sse.ts.\n";
for (const [name, schema] of Object.entries(schemas).toSorted(([a], [b]) => a.localeCompare(b))) {
	if (!isObject(schema)) continue;
	const variants = [schema["oneOf"], schema["anyOf"]].find(Array.isArray) as Json[] | undefined;
	if (!variants?.some((variant) => "x-sse-event" in variant)) continue;

	const mappings = variants.map((variant) => {
		const event = variant["x-sse-event"];
		const target = variant["$ref"];
		if (typeof event !== "string") throw new Error(`an SSE variant of ${name} needs x-sse-event`);
		if (typeof target !== "string" || !target.startsWith(reference))
			throw new Error(`the SSE variant ${event} of ${name} needs a named schema`);
		return { event, target: target.slice(reference.length) };
	});
	if (new Set(mappings.map(({ event }) => event)).size !== mappings.length)
		throw new Error(`${name} names an SSE event twice`);

	source += `export const ${name}Names = {\n`;
	for (const { event, target } of mappings) source += `\t${target}: ${JSON.stringify(event)},\n`;
	source += `} as const;\nexport type ${name}Data = {\n`;
	for (const { event, target } of mappings)
		source += `\t${event}: import("./generated").${target};\n`;
	source += "};\n";
}

writeFileSync(join(import.meta.dirname, "src/operator/sse.gen.ts"), source);
