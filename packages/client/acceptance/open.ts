// bun acceptance/open.ts <name> <agent> <profile> <model|-> <brief> [workspace]
import { chromium } from "@playwright/test";

const [name, agent, profile, model, brief, existing] = process.argv.slice(2);
if (!name || !agent || !profile || !model || !brief) throw new Error("arguments missing");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const operator = process.env.KESTREL_CONTROL_PLANE ?? "http://127.0.0.1:7738";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";
const say = (record: Record<string, unknown>) =>
	console.log(JSON.stringify({ at: new Date().toISOString(), ...record }));

const browser = await chromium.launch();
const context = await browser.newContext({ ignoreHTTPSErrors: true });
await context.addInitScript((participant) => {
	localStorage.setItem("kestrel:participant", participant);
}, name);
const page = await context.newPage();
if (existing) {
	await page.goto(`${base}/organizations/${organization}/workspaces/${existing}`);
} else {
	await page.goto(`${base}/organizations/${organization}/new`);
	await page.getByLabel("Agent").selectOption(agent);
	await page.getByRole("button", { name: "Options" }).click();
	if (model !== "-") await page.getByLabel("Model").fill(model);
	await page.getByLabel("Subscription Profile").selectOption(profile);
	say({ type: "form", queueLine: await page.locator("form").innerText() });
	await page.getByRole("button", { name: "Open Workspace" }).click();
	await page.waitForURL(/\/workspaces\/[a-z]+-[a-z]+-[a-z]+$/);
}
const workspace = page.url().split("/").at(-1) ?? "";
say({ type: "opened", workspace });

const queue = async () =>
	(await fetch(`${operator}/operator/organizations/${organization}/queue`)).json();
const message = page.getByLabel("Message");
for (let tick = 0; tick < 120; tick++) {
	const snapshot = (await queue()) as {
		active_work: { occupants: { name: string; workspace: string }[] };
		unbriefed: { workspace: string }[];
	};
	const header = await page
		.locator("[data-session-header]")
		.innerText()
		.catch(() => null);
	const transcript = await page
		.getByRole("region", { name: "Transcript" })
		.innerText()
		.catch(() => null);
	say({
		type: "getting-ready",
		header,
		transcript: transcript?.slice(0, 400),
		unbriefed: snapshot.unbriefed,
		occupants: snapshot.active_work.occupants.map((row) => row.name),
	});
	if (header?.includes("Waiting") || header?.includes("waiting")) break;
	await page.waitForTimeout(5_000);
}
await message.fill(brief);
await page.getByRole("button", { name: "Send" }).click();
await page.waitForTimeout(5_000);
say({
	type: "sent",
	header: await page
		.locator("[data-session-header]")
		.innerText()
		.catch(() => null),
});
await browser.close();
