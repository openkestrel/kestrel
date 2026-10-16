import { defineConfig } from "@hey-api/openapi-ts";
import { document } from "./operator-document";

// Types only: the transport owns every request, so refusals and streams behave as the CLI's do (ADR-0036).
export default defineConfig({
	input: document,
	output: { path: "src/operator/generated", postProcess: [] },
	plugins: ["@hey-api/typescript"],
});
