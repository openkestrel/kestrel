// bun acceptance/live.ts <workspace> [seconds]
import { chromium } from "@playwright/test";

type LiveEntry = { at: string; kind: string; detail: string };

const [workspace, seconds = "120"] = process.argv.slice(2);
if (!workspace) throw new Error("arguments missing");
const base = process.env.KESTREL_CLIENT_URL ?? "http://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";
const say = (record: Record<string, unknown>) =>
	console.log(JSON.stringify({ at: new Date().toISOString(), ...record }));

const browser = await chromium.launch();
const context = await browser.newContext();
await context.addInitScript(() => localStorage.setItem("kestrel:participant", "jack"));
const page = await context.newPage();

await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
await page.waitForTimeout(3_000);
const composer = page.locator("[data-composer-input]");
await composer.focus();
await page.keyboard.type("a draft that should keep its focus");

await page.evaluate(() => {
	const log: LiveEntry[] = [];
	(window as unknown as { liveLog: typeof log }).liveLog = log;
	const now = () => new Date().toISOString();
	document.addEventListener("focusin", (event) => {
		const target = event.target as HTMLElement;
		log.push({ at: now(), kind: "focus", detail: target.outerHTML.slice(0, 120) });
	});
	for (const region of document.querySelectorAll<HTMLElement>(
		"[aria-live=polite], [aria-live=assertive], [role=alert], [role=status]",
	)) {
		const name =
			region.dataset.transcriptAnnouncement !== undefined
				? "transcript"
				: region.dataset.announcement !== undefined
					? "composer"
					: (region.getAttribute("role") ?? region.getAttribute("aria-live") ?? "region");
		let last = region.innerText;
		new MutationObserver(() => {
			if (region.innerText === last) return;
			last = region.innerText;
			log.push({ at: now(), kind: `live:${name}`, detail: last.slice(0, 160) });
		}).observe(region, { childList: true, subtree: true, characterData: true });
	}
});

await page.waitForTimeout(Number(seconds) * 1_000);
const log = await page.evaluate(() => (window as unknown as { liveLog: LiveEntry[] }).liveLog);
const focused = await page.evaluate(() => ({
	composer: document.activeElement?.hasAttribute("data-composer-input") ?? false,
	draft: (document.querySelector("[data-composer-input]") as HTMLTextAreaElement | null)?.value,
}));
for (const entry of log) say(entry);
say({
	type: "summary",
	focusChanges: log.filter((entry) => entry.kind === "focus").length,
	announcements: log.filter((entry) => entry.kind.startsWith("live:")).length,
	...focused,
});
await browser.close();
