// bun acceptance/work.ts <workspace> <shots dir> [directory] [file]
import { chromium } from "@playwright/test";

const [workspace, shots, directory = "docs", file = "README.md"] = process.argv.slice(2);
if (!workspace || !shots) throw new Error("workspace and shots directory needed");
const base = process.env.KESTREL_CLIENT_URL ?? "https://127.0.0.1:7739";
const organization = process.env.KESTREL_ORGANIZATION ?? "acme";

const browser = await chromium.launch();
const context = await browser.newContext({
	ignoreHTTPSErrors: true,
	viewport: { width: 1440, height: 1000 },
});
const page = await context.newPage();
await page.goto(`${base}/organizations/${organization}/workspaces/${workspace}`);
const work = page.getByRole("region", { name: "Work", exact: true });
await work.waitFor();
await page.waitForTimeout(2_000);

const shown: Record<string, string> = {};
const capture = async (label: string) => {
	await page.waitForTimeout(2_000);
	shown[label] = (await work.innerText()).slice(0, 1_500);
	await page.screenshot({ path: `${shots}/${workspace}-${label}.png` });
};

await work.getByRole("tab", { name: "Work", exact: true }).click();
await capture("work");
await work.getByRole("tab", { name: "Diff", exact: true }).click();
for (const scope of ["Unpublished", "Changed", "Staged"]) {
	const toggle = work
		.getByRole("radio", { name: new RegExp(scope, "i") })
		.or(work.getByRole("button", { name: new RegExp(`^${scope}`, "i") }));
	if (
		await toggle
			.first()
			.isVisible()
			.catch(() => false)
	) {
		await toggle.first().click();
		await capture(`diff-${scope.toLowerCase()}`);
	}
}
await work.getByRole("tab", { name: "Files", exact: true }).click();
await capture("files-root");
for (const segment of [...directory.split("/"), ...file.split("/")]) {
	const entry = work.getByRole("button", { name: new RegExp(`^${segment}`) });
	if (
		await entry
			.first()
			.isVisible()
			.catch(() => false)
	)
		await entry.first().click();
	await page.waitForTimeout(1_000);
}
await capture("files-open");
await work.getByRole("tab", { name: "Sessions", exact: true }).click();
await capture("sessions");
console.log(JSON.stringify(shown, null, 2));
await browser.close();
