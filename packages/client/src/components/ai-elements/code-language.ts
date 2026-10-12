import type { BundledLanguage } from "shiki";
import type { CodeLanguage } from "./code-block";

const LANGUAGES: Record<string, BundledLanguage> = {
	css: "css",
	go: "go",
	html: "html",
	js: "javascript",
	json: "json",
	jsx: "jsx",
	md: "markdown",
	mjs: "javascript",
	py: "python",
	rs: "rust",
	sh: "shellscript",
	sql: "sql",
	toml: "toml",
	ts: "typescript",
	tsx: "tsx",
	yaml: "yaml",
	yml: "yaml",
};

export function languageFor(path: string): CodeLanguage {
	const name = path.slice(path.lastIndexOf("/") + 1);
	const dot = name.lastIndexOf(".");
	if (dot <= 0) return "plaintext";
	return LANGUAGES[name.slice(dot + 1).toLowerCase()] ?? "plaintext";
}
