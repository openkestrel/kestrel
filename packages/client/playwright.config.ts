import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { defineConfig, devices } from "@playwright/test";

const operator = 17718;

// The built Client, served by a real control plane's operator listener.
export default defineConfig({
	testDir: "e2e",
	forbidOnly: !!process.env.CI,
	reporter: process.env.CI ? "github" : "list",
	use: { baseURL: `http://127.0.0.1:${operator}`, trace: "retain-on-failure" },
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	webServer: {
		command: "cargo run --quiet --locked --package kestrel --bin kestrel-control-plane -- serve",
		cwd: resolve(import.meta.dirname, "../.."),
		url: `http://127.0.0.1:${operator}/operator/organizations`,
		timeout: 600_000,
		reuseExistingServer: false,
		env: {
			KESTREL_DATA_DIR: mkdtempSync(join(tmpdir(), "kestrel-client-e2e-")),
			KESTREL_LISTEN: "127.0.0.1:17717",
			KESTREL_OPERATOR_LISTEN: `127.0.0.1:${operator}`,
			KESTREL_CLIENT_DIR: resolve(import.meta.dirname, "dist/client"),
		},
	},
});
