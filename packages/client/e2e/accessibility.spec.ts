import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

test.beforeAll(async ({ request }) => {
	await request.post("/operator/organizations", { data: { name: "acme" } });
	await request.post("/operator/organizations/acme/projects", {
		data: {
			name: "kestrel",
			repositories: ["https://github.com/openkestrel/kestrel"],
			branch: "main",
		},
	});
	await request.post("/operator/organizations/acme/agents", {
		data: { name: "builder", harness: "opencode" },
	});
});

async function noSeriousViolation(page: Page): Promise<void> {
	const { violations } = await new AxeBuilder({ page }).analyze();
	const serious = violations.filter(({ impact }) => impact === "serious" || impact === "critical");
	expect(
		serious
			.map(
				({ id, help, nodes }) =>
					`${id}: ${help} (${nodes.length} node(s))\n${nodes
						.map((node) => `  ${node.target.join(" ")} :: ${node.html.slice(0, 160)}`)
						.join("\n")}`,
			)
			.join("\n"),
	).toBe("");
}

test("the Workspaces list has no serious violation at desktop and 375 px", async ({
	page,
	request,
}) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);

	await page.goto("/organizations/acme");
	await expect(page.getByRole("heading", { name: "Workspaces" })).toBeVisible();
	await noSeriousViolation(page);

	await page.setViewportSize({ width: 375, height: 667 });
	await expect(page.getByRole("tab", { name: "Workspaces" })).toBeVisible();
	await noSeriousViolation(page);
});

test("every Work pane view has no serious violation", async ({ page, request }) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await opened.json()) as { workspace: { name: string } };

	await page.goto(`/organizations/acme/workspaces/${body.workspace.name}`);
	await expect(page.getByRole("heading", { name: body.workspace.name })).toBeVisible();
	await noSeriousViolation(page);

	const work = page.getByRole("region", { name: "Work", exact: true });
	for (const tab of ["Diff", "Files", "Sessions", "People"]) {
		// oxlint-disable-next-line no-await-in-loop -- each view is audited after its tab is chosen.
		await auditView(work, page, tab);
	}
});

async function auditView(work: Locator, page: Page, tab: string): Promise<void> {
	await work.getByRole("tab", { name: tab }).click();
	await noSeriousViolation(page);
}

test("the Transcript announces shared-state entries and restrains tool chatter", async ({
	page,
	request,
}) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await opened.json()) as { workspace: { name: string } };

	await page.goto(`/organizations/acme/workspaces/${body.workspace.name}`);
	await expect(page.getByRole("heading", { name: body.workspace.name })).toBeVisible();

	await expect(page.getByRole("log")).toHaveAttribute("aria-live", "off");
	const announcer = page.locator("[data-transcript-announcement]");
	await expect(announcer).toHaveAttribute("aria-live", "polite");
	await expect(announcer).toHaveClass(/sr-only/);
});

test("the New Workspace form has no serious violation", async ({ page }) => {
	await page.goto("/organizations/acme/new");
	await expect(page.getByRole("heading", { name: "New Workspace" })).toBeVisible();
	await noSeriousViolation(page);

	await page.setViewportSize({ width: 375, height: 667 });
	await noSeriousViolation(page);
});
