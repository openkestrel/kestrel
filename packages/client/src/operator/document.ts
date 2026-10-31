import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type JsonObject = { [key: string]: Json };

const isObject = (value: Json | undefined): value is JsonObject =>
	typeof value === "object" && value !== null && !Array.isArray(value);

function target(reference: string, base: string): [string, string] {
	const hash = reference.indexOf("#");
	const file = hash < 0 ? reference : reference.slice(0, hash);
	const pointer = hash < 0 ? "" : reference.slice(hash + 1);
	return [file === "" ? base : realpathSync(join(dirname(base), file)), pointer];
}

/**
 * Reads the OpenAPI document rooted at `root` and the files its `$ref`s name as one document, as
 * `kestrel_operator_types::document::bundle` does: every other file a reference reaches outside the
 * root's `paths` is a map of named schemas, which all join the root's own `components.schemas`.
 */
export function bundle(root: string): JsonObject {
	root = realpathSync(root);
	const files = new Map<string, Json>();
	const owners = new Map<string, string>();
	const hoisted = new Set<string>();
	const schemas: JsonObject = {};

	const load = (file: string): Json => {
		const cached = files.get(file);
		if (cached !== undefined) return cached;
		const value: Json = JSON.parse(readFileSync(file, "utf8"));
		files.set(file, value);
		return value;
	};

	const walk = (value: Json, base: string): Json => {
		if (Array.isArray(value)) return value.map((item) => walk(item, base));
		if (!isObject(value)) return value;
		return Object.fromEntries(
			Object.entries(value).map(([key, member]) => [
				key,
				key === "$ref" && typeof member === "string" ? reference(member, base) : walk(member, base),
			]),
		);
	};

	const reference = (ref: string, base: string): string => {
		const [file, pointer] = target(ref, base);
		if (file === root) return `#${pointer}`;
		const name = pointer.startsWith("/") ? pointer.slice(1) : "";
		if (name === "" || name.includes("/"))
			throw new Error(`${base} refers to ${ref}, not a named schema`);
		hoist(file);
		if (owners.get(name) !== file) throw new Error(`${base} refers to ${ref}, which is not there`);
		return `#/components/schemas/${name}`;
	};

	const hoist = (file: string) => {
		if (hoisted.has(file)) return;
		hoisted.add(file);
		const named = load(file);
		if (!isObject(named)) throw new Error(`${file} is not a map of named schemas`);
		for (const name of Object.keys(named)) {
			const owner = owners.get(name);
			if (owner !== undefined)
				throw new Error(`${file} names the schema ${name}, which ${owner} already names`);
			owners.set(name, file);
		}
		for (const [name, schema] of Object.entries(named)) schemas[name] = walk(schema, file);
	};

	const at = (value: Json, pointer: string, file: string): Json => {
		let found: Json | undefined = value;
		for (const token of pointer.split("/").slice(1)) {
			const key = token.replaceAll("~1", "/").replaceAll("~0", "~");
			found = Array.isArray(found) ? found[Number(key)] : isObject(found) ? found[key] : undefined;
		}
		if (found === undefined) throw new Error(`${file} has nothing at ${pointer}`);
		return found;
	};

	const document = load(root);
	if (!isObject(document)) throw new Error(`${root} is not an object`);
	const bundled: JsonObject = {};
	for (const [key, value] of Object.entries(document)) {
		if (key !== "paths") {
			bundled[key] = walk(value, root);
			continue;
		}
		if (!isObject(value)) throw new Error(`${root} has paths that are not an object`);
		bundled.paths = Object.fromEntries(
			Object.entries(value).map(([path, item]) => {
				const ref = isObject(item) ? item.$ref : undefined;
				if (typeof ref !== "string") return [path, walk(item, root)];
				const [file, pointer] = target(ref, root);
				return [path, walk(at(load(file), pointer, file), file)];
			}),
		);
	}
	const components = (bundled.components ??= {});
	if (!isObject(components)) throw new Error(`${root} has components that are not an object`);
	const own = (components.schemas ??= {});
	if (!isObject(own)) throw new Error(`${root} has schemas that are not an object`);
	for (const [name, schema] of Object.entries(schemas)) {
		if (name in own)
			throw new Error(`${owners.get(name)} names the schema ${name}, which the root already names`);
		own[name] = schema;
	}
	return bundled;
}
