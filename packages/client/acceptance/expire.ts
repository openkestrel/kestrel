// bun acceptance/expire.ts <workspace> <activity-first-seq> <entry-seq> <flag-file>
// Write the flag file once the entries have expired; the open page then loads the reference it held.
import { existsSync } from "node:fs";
import { chromium, type Page } from "@playwright/test";

const [workspace, activitySeq, entrySeq, flag] = process.argv.slice(2);
if (!workspace || !activitySeq || !entrySeq || !flag) throw new Error("arguments missing");
const base = process.env.KESTREL_CLIENT_URL ?? "http://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";
const say = (record: Record<string, unknown>) =>
	console.log(JSON.stringify({ at: new Date().toISOString(), ...record }));

async function openActivity(page: Page) {
	await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
	const activity = page.locator(`[data-activity="${activitySeq}"]`);
	await activity.waitFor({ timeout: 30_000 });
	await activity.locator("button").first().focus();
	await page.keyboard.press("Enter");
	await page.waitForTimeout(2_000);
	const entry = activity.locator(`article[data-seq="${entrySeq}"]`);
	const more = entry.getByRole("button", { name: "More" });
	if (await more.isVisible().catch(() => false)) {
		await more.focus();
		await page.keyboard.press("Enter");
		await page.waitForTimeout(1_000);
	}
	return entry;
}

const browser = await chromium.launch();
const context = await browser.newContext();
await context.addInitScript(() => localStorage.setItem("kestrel:participant", "jack"));
const page = await context.newPage();

const before = await openActivity(page);
const load = before.getByRole("button", { name: /^Load .* payload$/ });
say({ type: "before", references: await load.allInnerTexts() });

while (!existsSync(flag)) await page.waitForTimeout(1_000);
say({ type: "flag" });
await page.waitForTimeout(5_000);
await load.first().focus();
await page.keyboard.press("Enter");
await page.waitForTimeout(3_000);
say({ type: "loaded-after-expiry", text: (await before.innerText()).slice(0, 1500) });

const after = await openActivity(page);
say({ type: "reopened", text: (await after.innerText()).slice(0, 1500) });
await browser.close();
