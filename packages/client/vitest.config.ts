import tailwindcss from "@tailwindcss/vite";
import viteReact from "@vitejs/plugin-react";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig } from "vitest/config";

export default defineConfig({
	plugins: [tailwindcss(), viteReact()],
	resolve: { tsconfigPaths: true },
	test: {
		projects: [
			{ test: { name: "unit", include: ["src/**/*.test.ts"] } },
			{
				test: {
					name: "browser",
					include: ["component-tests/**/*.test.tsx"],
					browser: {
						enabled: true,
						headless: true,
						provider: playwright(),
						instances: [{ browser: "chromium", viewport: { width: 1280, height: 800 } }],
					},
				},
			},
		],
	},
});
