// bun acceptance/act.ts <workspace> <name> brief|post|send-now|interrupt|edit|withdraw|look [text]
import { chromium } from "@playwright/test";

const [workspace, name, action, text = ""] = process.argv.slice(2);
if (!workspace || !name || !action) throw new Error("workspace, name and action needed");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";

const browser = await chromium.launch();
const context = await browser.newContext({ ignoreHTTPSErrors: true });
await context.addInitScript((participant) => {
	localStorage.setItem("kestrel:participant", participant);
}, name);
const page = await context.newPage();
await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
if (action === "brief") {
	await page.getByLabel("Message").fill(text);
	await page.screenshot({
		path: `${process.env.KESTREL_SHOTS ?? "."}/${workspace}-before-brief.png`,
		fullPage: true,
	});
	await page.getByRole("button", { name: "Send" }).click();
	await page.waitForTimeout(5_000);
	console.log(
		JSON.stringify({ action, header: await page.locator("[data-session-header]").innerText() }),
	);
	await browser.close();
	process.exit(0);
}
const composer = page.locator("[data-composer]");
await composer.waitFor();
await page.waitForTimeout(1_500);

const input = composer.locator("[data-composer-input]");
const authored = composer.locator("[data-held-message]").filter({ hasText: name });
switch (action) {
	case "post":
		await input.fill(text);
		await composer.locator('button[type="submit"]').click();
		break;
	case "send-now":
		await input.fill(text);
		await composer.getByRole("button", { name: "Send now" }).click();
		break;
	case "interrupt":
		await composer.getByRole("button", { name: "Interrupt" }).click();
		break;
	case "edit":
		await authored.last().getByRole("button", { name: "Edit" }).click();
		await composer.getByLabel("Edit the held message").fill(text);
		await composer.getByRole("button", { name: "Save" }).click();
		break;
	case "withdraw":
		await authored.last().getByRole("button", { name: "Withdraw" }).click();
		break;
	case "look":
		break;
	default:
		throw new Error(`no action ${action}`);
}
await page.waitForTimeout(2_500);
const alerts = await page.getByRole("alert").allInnerTexts();
console.log(
	JSON.stringify({
		action,
		submitLabel: await composer
			.locator('button[type="submit"]')
			.innerText()
			.catch(() => null),
		held: await composer.locator("[data-held-message]").allInnerTexts(),
		state: await page
			.locator("[data-session-state]")
			.innerText()
			.catch(() => null),
		interrupting: await page
			.locator("[data-session-interrupting]")
			.innerText()
			.catch(() => null),
		alerts,
	}),
);
await browser.close();
