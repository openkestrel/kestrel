import { defineConfig } from "@hey-api/openapi-ts";

// Types only: the transport owns every request, so refusals and streams behave as the CLI's do (ADR-0036).
export default defineConfig({
	input: "../../openapi/operator.json",
	output: { path: "src/operator/generated", postProcess: [] },
	plugins: ["@hey-api/typescript"],
});
