// bun acceptance/queue.ts <kestrel binary>
import { execFileSync } from "node:child_process";
import { chromium } from "@playwright/test";

const [binary] = process.argv.slice(2);
if (!binary) throw new Error("the kestrel binary is needed");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";

const browser = await chromium.launch();
const page = await browser.newPage({ ignoreHTTPSErrors: true });
await page.goto(`${base}/organizations/${organization}`);
await page.locator("[data-queue-header]").waitFor();
await page.waitForTimeout(1_500);
const header = await page.locator("[data-queue-header]").innerText();
const rows = await page.locator('nav[aria-label="Workspaces"] li').allInnerTexts();
const lines: Record<string, string | null> = {};
for (const link of await page.locator('nav[aria-label="Workspaces"] a').all()) {
	const href = (await link.getAttribute("href")) ?? "";
	const name = href.split("/").at(-1) ?? href;
	const workspace = await browser.newPage({ ignoreHTTPSErrors: true });
	await workspace.goto(`${base}${href}`);
	await workspace.waitForTimeout(2_000);
	const line = workspace.locator("[data-queue-line]");
	lines[name] = (await line.count()) > 0 ? await line.innerText() : null;
	await workspace.close();
}
const cli = execFileSync(binary, ["queue"], { encoding: "utf8" });
const operatorRead = execFileSync(
	"curl",
	[
		"-sf",
		`${process.env.KESTREL_CONTROL_PLANE ?? "http://127.0.0.1:7738"}/operator/organizations/${organization}/queue`,
	],
	{ encoding: "utf8" },
);
console.log(
	JSON.stringify(
		{
			at: new Date().toISOString(),
			browser: { header, rows, lines },
			cli,
			operator: JSON.parse(operatorRead),
		},
		null,
		2,
	),
);
await browser.close();
