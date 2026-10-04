// bun acceptance/keyboard.ts <width>
import { chromium, type Locator, type Page } from "@playwright/test";

const width = Number(process.argv[2] ?? "1280");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";
let presses = 0;
const steps: string[] = [];

async function press(page: Page, key: string): Promise<void> {
	presses++;
	await page.keyboard.press(key);
}

async function tabTo(page: Page, target: Locator, what: string, tries = 300): Promise<void> {
	for (let index = 0; index < tries; index += 1) {
		await press(page, "Tab");
		if (await target.evaluate((element) => element === document.activeElement).catch(() => false)) {
			steps.push(`${what}: ${index + 1} Tab`);
			return;
		}
	}
	throw new Error(`the keyboard path never reached ${what}`);
}

const browser = await chromium.launch();
const context = await browser.newContext({
	ignoreHTTPSErrors: true,
	viewport: { width, height: width < 900 ? 667 : 900 },
});
const page = await context.newPage();
await page.goto(`${base}/organizations/${organization}`);
const row = page.locator('nav[aria-label="Workspaces"] a', { hasText: "Working" }).first();
await row.waitFor();
const workspace = ((await row.getAttribute("href")) ?? "").split("/").at(-1) ?? "";
await tabTo(page, row, `the running Session row (${workspace})`);
await press(page, "Enter");
await page.waitForURL(new RegExp(`/workspaces/${workspace}$`));

const panes = page.getByRole("tablist", { name: "Panes" });
const narrow = await panes.isVisible();
if (narrow) {
	await tabTo(page, panes.getByRole("tab", { selected: true }), "the selected area tab");
	await press(page, "ArrowRight");
	await press(page, "Enter");
}
const work = page.getByRole("region", { name: "Work", exact: true });
const views = work.getByRole("tablist", { name: "Work views" });
await tabTo(page, views.getByRole("tab", { selected: true }), "the selected Work view tab");
await press(page, "ArrowRight");
await press(page, "Enter");
await page.waitForTimeout(2_500);
const diffView = await work.innerText();
steps.push(`diff view: ${diffView.slice(0, 120).replace(/\n/g, " ")}`);

if (narrow) {
	await tabTo(page, panes.getByRole("tab", { selected: true }), "the selected area tab");
	await press(page, "ArrowLeft");
	await press(page, "Enter");
}
const composer = page.locator("[data-composer-input]");
await tabTo(page, composer, "the composer");
await page.keyboard.type("keyboard acceptance check: please ignore, withdrawn at once");
const submit = page.locator('[data-composer] button[type="submit"]');
await tabTo(page, submit, "the submit button");
await press(page, "Enter");
const name = page.getByLabel("Your name");
await name.waitFor({ timeout: 5_000 });
await tabTo(page, name, "the name gate");
await page.keyboard.type("keyboard");
await press(page, "Enter");
await tabTo(page, submit, "the submit button again");
await press(page, "Enter");
await page.waitForTimeout(3_000);
const held = page.locator("[data-held-message]").filter({ hasText: "keyboard acceptance check" });
const posted =
	(await held.count()) > 0 ||
	(await page.getByRole("log").getByText("keyboard: keyboard acceptance check").count()) > 0;
steps.push(`turn taken: ${posted} (held: ${await held.count()})`);
if ((await held.count()) > 0) {
	await tabTo(page, held.getByRole("button", { name: "Withdraw" }), "Withdraw");
	await press(page, "Enter");
	await page.waitForTimeout(2_000);
	steps.push(`withdrawn: ${(await held.count()) === 0}`);
}
const scroll = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
console.log(
	JSON.stringify({ width, workspace, narrow, presses, steps, horizontalScroll: scroll }, null, 2),
);
await browser.close();
