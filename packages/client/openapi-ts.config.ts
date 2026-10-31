import { fileURLToPath } from "node:url";
import { defineConfig } from "@hey-api/openapi-ts";
import { bundle } from "./src/operator/document";

// Types only: the transport owns every request, so refusals and streams behave as the CLI's do (ADR-0036).
export default defineConfig({
	input: { path: bundle(fileURLToPath(new URL("../../openapi/operator.json", import.meta.url))) },
	output: { path: "src/operator/generated", postProcess: [] },
	plugins: ["@hey-api/typescript"],
});
