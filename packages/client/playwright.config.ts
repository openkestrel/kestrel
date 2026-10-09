import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { defineConfig, devices } from "@playwright/test";

const operator = 17718;
const client = 17719;

export default defineConfig({
	testDir: "e2e",
	forbidOnly: !!process.env.CI,
	reporter: process.env.CI ? "github" : "list",
	use: {
		baseURL: `http://127.0.0.1:${client}`,
		trace: "retain-on-failure",
	},
	projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
	webServer: [
		{
			command: "cargo run --quiet --locked --package kestrel --bin kestrel-control-plane -- serve",
			cwd: resolve(import.meta.dirname, "../.."),
			url: `http://127.0.0.1:${operator}/operator/organizations`,
			timeout: 600_000,
			reuseExistingServer: false,
			env: {
				KESTREL_DATA_DIR: mkdtempSync(join(tmpdir(), "kestrel-client-e2e-")),
				KESTREL_LISTEN: "127.0.0.1:17717",
				KESTREL_OPERATOR_LISTEN: `127.0.0.1:${operator}`,
			},
		},
		{
			command: "caddy run --adapter caddyfile --config ../../images/kestrel-client/Caddyfile",
			url: `http://127.0.0.1:${client}/`,
			reuseExistingServer: false,
			env: {
				XDG_DATA_HOME: mkdtempSync(join(tmpdir(), "kestrel-client-e2e-caddy-")),
				KESTREL_CLIENT_LISTEN_PORT: String(client),
				KESTREL_CLIENT_ROOT: resolve(import.meta.dirname, "dist/client"),
				KESTREL_CONTROL_PLANE: `http://127.0.0.1:${operator}`,
			},
		},
	],
});
