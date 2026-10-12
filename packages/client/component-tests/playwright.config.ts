import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: ".",
	forbidOnly: !!process.env.CI,
	workers: 1,
	use: { baseURL: "http://127.0.0.1:17828", trace: "retain-on-failure" },
	webServer: {
		cwd: new URL("..", import.meta.url).pathname,
		command:
			"bunx --no-install vite --config component-tests/vite.config.ts --host 127.0.0.1 --port 17828 --strictPort",
		url: "http://127.0.0.1:17828/component-tests/approval-question.html",
		reuseExistingServer: false,
	},
});
