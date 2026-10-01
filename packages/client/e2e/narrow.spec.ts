import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";

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

async function opened(request: APIRequestContext): Promise<{ id: string; name: string }> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return body.workspace;
}

async function noHorizontalScroll(page: Page): Promise<void> {
	const width = await page.evaluate(() => ({
		document: document.documentElement.scrollWidth,
		body: document.body.scrollWidth,
		viewport: window.innerWidth,
	}));
	expect(width.document, "the page scrolls horizontally").toBeLessThanOrEqual(width.viewport);
	expect(width.body, "the body scrolls horizontally").toBeLessThanOrEqual(width.viewport);
}

test("at 375 px the three areas are tabs, and no view scrolls sideways", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 375, height: 667 });
	const workspace = await opened(request);

	await page.goto("/organizations/acme");
	await expect(page.getByRole("tab", { name: "Workspaces" })).toBeVisible();
	await noHorizontalScroll(page);

	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
	const panes = page.getByRole("tablist", { name: "Panes" });
	for (const pane of ["Transcript", "Workspaces", "Work"]) {
		// oxlint-disable-next-line no-await-in-loop -- each area is shown before it is measured.
		await showPane(panes, page, pane);
	}

	await page.goto("/organizations/acme/new");
	await expect(page.getByRole("heading", { name: "New Workspace" })).toBeVisible();
	await noHorizontalScroll(page);
});

async function showPane(panes: Locator, page: Page, pane: string): Promise<void> {
	await panes.getByRole("tab", { name: pane, exact: true }).click();
	await expect(page.getByRole("region", { name: pane, exact: true })).toBeVisible();
	await noHorizontalScroll(page);
}

test("at 900 px the panes show side by side, and at 899 px they are tabs", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const panes = page.getByRole("tablist", { name: "Panes" });

	await page.setViewportSize({ width: 899, height: 700 });
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);
	await expect(panes).toBeVisible();
	await expect(panes.getByRole("tab", { name: "Transcript" })).toBeVisible();

	await page.setViewportSize({ width: 900, height: 700 });
	await expect(panes).toBeHidden();
	await Promise.all(
		["Workspaces", "Transcript", "Work"].map((pane) =>
			expect(page.getByRole("region", { name: pane, exact: true })).toBeVisible(),
		),
	);
});

test("selection and focus survive a change of area", async ({ page, request }) => {
	await page.setViewportSize({ width: 375, height: 667 });
	const workspace = await opened(request);
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);

	// Choose a Diff scope, then leave and come back by keyboard.
	const panes = page.getByRole("tablist", { name: "Panes" });
	await panes.getByRole("tab", { name: "Work", exact: true }).click();
	const work = page.getByRole("region", { name: "Work", exact: true });
	await work.getByRole("tab", { name: "Diff" }).click();
	await work.getByRole("button", { name: "Changed" }).click();
	await expect(work.getByRole("button", { name: "Changed" })).toHaveAttribute(
		"aria-pressed",
		"true",
	);

	await panes.getByRole("tab", { name: "Transcript" }).focus();
	await page.keyboard.press("Enter");
	await expect(panes.getByRole("tab", { name: "Transcript" })).toBeFocused();

	await panes.getByRole("tab", { name: "Work", exact: true }).focus();
	await page.keyboard.press("Enter");
	await expect(panes.getByRole("tab", { name: "Work", exact: true })).toBeFocused();
	await expect(work.getByRole("tab", { name: "Diff" })).toHaveAttribute("aria-selected", "true");
	await expect(work.getByRole("button", { name: "Changed" })).toHaveAttribute(
		"aria-pressed",
		"true",
	);
});
