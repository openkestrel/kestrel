// bun acceptance/axe.ts <workspace>...
import { AxeBuilder } from "@axe-core/playwright";
import { chromium, type Locator, type Page } from "@playwright/test";

const workspaces = process.argv.slice(2);
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";

type Finding = {
	view: string;
	width: number;
	id: string;
	impact: string | null;
	nodes: number;
	first: string;
};
const findings: Finding[] = [];
const scrolled: string[] = [];
const blocked: string[] = [];
let audits = 0;

async function tap(locator: Locator, label: string): Promise<boolean> {
	try {
		await locator.click({ timeout: 5_000 });
		return true;
	} catch (error) {
		blocked.push(`${label}: ${String(error).split("\n")[0]}`);
		return false;
	}
}

async function audit(page: Page, view: string): Promise<void> {
	await page.waitForTimeout(800);
	const width = page.viewportSize()?.width ?? 0;
	const { violations } = await new AxeBuilder({ page }).analyze();
	audits++;
	for (const violation of violations) {
		findings.push({
			view,
			width,
			id: violation.id,
			impact: violation.impact ?? null,
			nodes: violation.nodes.length,
			first: `${violation.nodes[0]?.target.join(" ")} :: ${violation.nodes[0]?.html.slice(0, 240)}`,
		});
	}
	if (await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)) {
		scrolled.push(`${view} @ ${width}`);
	}
}

const browser = await chromium.launch();
for (const width of [1280, 375]) {
	const context = await browser.newContext({
		ignoreHTTPSErrors: true,
		viewport: { width, height: width === 375 ? 667 : 800 },
	});
	const page = await context.newPage();
	await page.addInitScript(() => localStorage.setItem("kestrel:participant", "jack"));
	await page.goto(`${base}/organizations/${organization}`);
	await audit(page, "workspaces");
	await page.goto(`${base}/organizations/${organization}/new`);
	await audit(page, "new");
	for (const workspace of workspaces) {
		await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
		await page.locator("[data-session-header], [data-composer]").first().waitFor();
		if (width === 375) {
			for (const pane of ["Transcript", "Work"]) {
				const tab = page.getByRole("tab", { name: pane, exact: true }).first();
				if (await tab.isVisible()) await tap(tab, `${workspace} ${pane} pane @ ${width}`);
				await audit(page, `${workspace} ${pane} pane`);
			}
			await tap(
				page.getByRole("tab", { name: "Transcript", exact: true }).first(),
				`back to Transcript @ ${width}`,
			);
		}
		for (const mode of ["Steps", "Full"]) {
			const toggle = page
				.getByRole("radio", { name: mode })
				.or(page.getByRole("button", { name: mode }));
			if (await toggle.first().isVisible()) {
				await tap(toggle.first(), `${workspace} ${mode} @ ${width}`);
				const activity = page.locator("[data-activity] button").first();
				if (await activity.isVisible().catch(() => false)) {
					// The Transcript keeps scrolling under a pointer while it sticks to the bottom.
					await activity.focus();
					await page.keyboard.press("Enter");
				}
				await audit(page, `${workspace} transcript ${mode}`);
			}
		}
		if (width === 375)
			await tap(
				page.getByRole("tab", { name: "Work", exact: true }).first(),
				`Work pane @ ${width}`,
			);
		const work = page.getByRole("region", { name: "Work", exact: true });
		for (const tab of ["Work", "Diff", "Files", "Sessions", "People"]) {
			const trigger = work.getByRole("tab", { name: tab, exact: true });
			if (await trigger.isVisible().catch(() => false)) {
				await tap(trigger, `${workspace} work ${tab} @ ${width}`);
				await audit(page, `${workspace} work ${tab}`);
			}
		}
	}
	await context.close();
}
await browser.close();
const serious = findings.filter(({ impact }) => impact === "serious" || impact === "critical");
console.log(
	JSON.stringify(
		{ audits, blocked, serious, other: findings.filter((f) => !serious.includes(f)), scrolled },
		null,
		2,
	),
);
