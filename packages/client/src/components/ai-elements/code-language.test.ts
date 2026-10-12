import { describe, expect, it } from "vitest";
import { languageFor } from "./code-language";

describe("a file's language", () => {
	it("reads it from the extension, and falls back to plain text", () => {
		expect(languageFor("crates/kestrel/src/operator.rs")).toBe("rust");
		expect(languageFor("src/components/workbench/work-pane.tsx")).toBe("tsx");
		expect(languageFor("README.md")).toBe("markdown");
		expect(languageFor("scripts/run.sh")).toBe("shellscript");
		expect(languageFor("Dockerfile")).toBe("plaintext");
	});
});
