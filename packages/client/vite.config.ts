import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import viteReact from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
	resolve: { tsconfigPaths: true },
	// The build prerenders the shell through this server, and in a container `localhost` can bind
	// an address its crawler never dials.
	preview: { host: "127.0.0.1" },
	server: { proxy: { "/operator": "http://127.0.0.1:7718" } },
	plugins: [
		tailwindcss(),
		tanstackStart({ spa: { enabled: true, prerender: { outputPath: "/index" } } }),
		viteReact(),
	],
});
