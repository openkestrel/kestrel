// bun acceptance/follow.ts <workspace> <participant> <out.jsonl> [minutes]
import { appendFileSync } from "node:fs";
import { chromium } from "@playwright/test";

const [workspace, participant, out, minutes = "240"] = process.argv.slice(2);
if (!workspace || !participant || !out) throw new Error("workspace, participant and out needed");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";

const log = (record: Record<string, unknown>) =>
	appendFileSync(out, `${JSON.stringify({ at: new Date().toISOString(), ...record })}\n`);

const browser = await chromium.launch();
const context = await browser.newContext({ ignoreHTTPSErrors: true });
await context.exposeFunction("__kestrelStream", (event: string, id: string, data: string) => {
	let parsed: unknown = data;
	try {
		parsed = JSON.parse(data);
	} catch {}
	log({ type: "stream", event, id, data: parsed });
});
await context.addInitScript(
	({ name }) => {
		localStorage.setItem("kestrel:participant", name);
		const original = window.fetch.bind(window);
		window.fetch = async (input, init) => {
			const response = await original(input, init);
			const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
			if (
				!response.body ||
				!(response.headers.get("content-type") ?? "").includes("event-stream")
			) {
				return response;
			}
			if (!url.includes("/transcript")) return response;
			const [mine, theirs] = response.body.tee();
			void (async () => {
				const reader = mine.pipeThrough(new TextDecoderStream()).getReader();
				let buffer = "";
				for (;;) {
					const { value, done } = await reader.read().catch(() => ({ value: "", done: true }));
					if (done) break;
					buffer += value;
					let cut = buffer.indexOf("\n\n");
					while (cut >= 0) {
						const block = buffer.slice(0, cut);
						buffer = buffer.slice(cut + 2);
						let event = "message";
						let id = "";
						const data: string[] = [];
						for (const line of block.split("\n")) {
							if (line.startsWith("event:")) event = line.slice(6).trim();
							else if (line.startsWith("id:")) id = line.slice(3).trim();
							else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
						}
						// @ts-expect-error exposed by the follower
						void window.__kestrelStream(event, id, data.join("\n"));
						cut = buffer.indexOf("\n\n");
					}
				}
			})();
			return new Response(theirs, {
				status: response.status,
				statusText: response.statusText,
				headers: response.headers,
			});
		};
	},
	{ name: participant },
);

const page = await context.newPage();
page.on("console", (message) => {
	if (message.type() === "error") log({ type: "console", text: message.text() });
});
await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
log({ type: "opened", workspace, participant });

const header = async () =>
	page
		.evaluate(() => {
			const text = (selector: string) =>
				document.querySelector(selector)?.textContent?.trim() ?? null;
			return {
				title: text("[data-session-title]"),
				state: text("[data-session-state]"),
				model: text("[data-session-model]"),
				usage: text("[data-session-usage]"),
				followers: text("[data-session-followers]"),
				commands: text("[data-session-commands]"),
				queue: text("[data-queue-line]"),
				live: text("[data-live]"),
				options: [...document.querySelectorAll("[data-option]")].map((node) =>
					node.textContent?.trim(),
				),
				horizontalScroll: document.documentElement.scrollWidth > window.innerWidth,
			};
		})
		.catch((error: unknown) => ({ error: String(error) }));

const until = Date.now() + Number(minutes) * 60_000;
while (Date.now() < until) {
	log({ type: "header", ...(await header()) });
	await page.waitForTimeout(15_000);
}
await browser.close();
