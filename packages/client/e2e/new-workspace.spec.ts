import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type APIRequestContext, type Page, type Route } from "@playwright/test";

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

const FORM = "/organizations/acme/new";
const OPENED = /\/organizations\/acme\/workspaces\/[^/]+$/;

type Workspace = { id: string; name: string };

async function opened(request: APIRequestContext): Promise<Workspace> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return { id: body.workspace.id, name: body.workspace.name };
}

async function named(page: Page, name: string): Promise<void> {
	await page.getByLabel("Your name").fill(name);
}

async function draftedByJack(page: Page, brief: string): Promise<void> {
	await page.goto(FORM);
	await page.getByLabel("Brief").fill(brief);
	await named(page, "jack");
}

async function stubOpen(page: Page, answer: (route: Route) => Promise<void>): Promise<void> {
	await page.route(
		(url) => url.pathname === "/operator/organizations/acme/workspaces",
		async (route) => {
			if (route.request().method() !== "POST") return route.continue();
			await answer(route);
		},
	);
}

test("the only Project and Agent are preselected, with the resolved values beside them", async ({
	page,
}) => {
	await page.goto(FORM);

	await expect(page.getByRole("heading", { name: "New Workspace" })).toBeVisible();
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");
	await expect(page.getByLabel("Agent")).toHaveValue("builder");
	await expect(
		page.getByText("https://github.com/openkestrel/kestrel", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("main", { exact: true })).toBeVisible();
	await expect(page.getByText("opencode", { exact: true })).toBeVisible();
	await expect(page.getByText("the harness's default", { exact: true })).toBeVisible();
	await expect(
		page.getByText("No dispatch configuration is recorded.", { exact: true }),
	).toBeVisible();
	await expect(
		page.getByText("No dispatch configuration is recorded, so queue order is unknown.", {
			exact: true,
		}),
	).toBeVisible();
});

test("the form opens with no refusal alert", async ({ page }) => {
	await page.goto(FORM);
	await expect(page.getByRole("heading", { name: "New Workspace" })).toBeVisible();
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");

	await expect(page.getByRole("alert")).toHaveCount(0);
});

test("the Options disclosure opens by keyboard and holds the override, Profile and branch", async ({
	page,
}) => {
	await page.goto(FORM);

	const options = page.getByRole("button", { name: "Options" });
	await expect(options).toHaveAttribute("aria-expanded", "false");
	await options.press("Enter");
	await expect(options).toHaveAttribute("aria-expanded", "true");

	await expect(page.getByLabel("Branch")).toHaveAttribute("placeholder", "kestrel/<workspace>");
	await page.getByLabel("Model").fill("scripted-max");
	await expect(page.getByText("scripted-max", { exact: true })).toBeVisible();
});

test("a refusal lands next to its field, opens the disclosure, and keeps every input", async ({
	page,
}) => {
	await draftedByJack(page, "Ship the parser.");
	await stubOpen(page, (route) =>
		route.fulfill({
			status: 422,
			contentType: "application/json",
			body: JSON.stringify({ message: "that branch is taken", field: "branch" }),
		}),
	);

	await page.getByRole("button", { name: "Open Workspace" }).click();

	const branch = page.getByLabel("Branch");
	await expect(branch).toBeVisible();
	await expect(branch).toHaveAttribute("aria-invalid", "true");
	await expect(page.locator("#new-workspace-branch-error")).toHaveText("that branch is taken");
	await expect(page.getByRole("alert")).toHaveCount(1);
	await expect(page.getByLabel("Brief")).toHaveValue("Ship the parser.");
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");
	await expect(page).toHaveURL(/\/organizations\/acme\/new$/);
});

test("a refusal with no field lands at the top, and keeps every input", async ({ page }) => {
	await draftedByJack(page, "Ship the parser.");
	await stubOpen(page, (route) =>
		route.fulfill({
			status: 422,
			contentType: "application/json",
			body: JSON.stringify({ message: "the queue is full" }),
		}),
	);

	await page.getByRole("button", { name: "Open Workspace" }).click();

	await expect(page.getByRole("alert")).toHaveCount(1);
	await expect(page.getByRole("alert")).toContainText("the queue is full");
	await expect(page.getByLabel("Brief")).toHaveValue("Ship the parser.");
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");
	await expect(page).toHaveURL(/\/organizations\/acme\/new$/);
});

test("a transport failure lands at the top, and keeps every input", async ({ page }) => {
	await draftedByJack(page, "Ship the parser.");
	await stubOpen(page, (route) => route.abort("failed"));

	await page.getByRole("button", { name: "Open Workspace" }).click();

	await expect(page.getByRole("alert")).toHaveCount(1);
	await expect(page.getByRole("alert")).toContainText("the control plane could not be reached");
	await expect(page.getByLabel("Brief")).toHaveValue("Ship the parser.");
	await expect(page).toHaveURL(/\/organizations\/acme\/new$/);
});

test("a dropped file is read into the Brief, and nothing is uploaded", async ({ page }) => {
	await page.goto(FORM);

	const posted: string[] = [];
	page.on("request", (request) => {
		if (request.method() === "POST") posted.push(request.url());
	});

	const brief = page.getByLabel("Brief");
	const dropped = await page.evaluateHandle(() => {
		const transfer = new DataTransfer();
		transfer.items.add(new File(["the dropped brief\n"], "brief.md", { type: "text/markdown" }));
		return transfer;
	});
	await brief.dispatchEvent("drop", { dataTransfer: dropped });

	await expect(brief).toHaveValue("the dropped brief\n");
	expect(posted).toEqual([]);
});

test("a Brief is written under a name: a browser with none declared is asked, and remembers it", async ({
	page,
}) => {
	await page.goto(FORM);

	await expect(page.getByLabel("Your name")).toBeVisible();
	await page.getByLabel("Brief").fill("Name me.");
	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page.locator("#new-workspace-name-error")).toHaveText(
		"Your name is needed before sending.",
	);
	await expect(page).toHaveURL(/\/organizations\/acme\/new$/);

	await named(page, "jack");
	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page).toHaveURL(OPENED);

	await page.getByRole("link", { name: "New Workspace" }).click();
	await expect(page.getByLabel("Your name")).toBeHidden();
});

test("an empty or whitespace-only Brief opens unbriefed, with no Participant", async ({ page }) => {
	const declarations: unknown[] = [];
	page.on("request", (request) => {
		if (request.method() === "POST" && request.url().endsWith("/workspaces")) {
			declarations.push(request.postDataJSON());
		}
	});

	await page.goto(FORM);

	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page).toHaveURL(OPENED);
	await expect(page.getByPlaceholder("Write the Brief…")).toBeVisible();

	await page.getByRole("link", { name: "New Workspace" }).click();
	await page.getByLabel("Brief").fill("   ");
	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page).toHaveURL(OPENED);

	await page.getByRole("link", { name: "New Workspace" }).click();
	await named(page, "jack");
	await page.getByLabel("Brief").fill("Follow me.");
	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page).toHaveURL(OPENED);

	expect(declarations).toEqual([
		expect.objectContaining({ brief: null, participant: null }),
		expect.objectContaining({ brief: null, participant: null }),
		expect.objectContaining({ brief: "Follow me.", participant: "jack" }),
	]);
});

test("opening lands on the new Workspace, queued for its turn, and follows it", async ({
	page,
}) => {
	await page.goto(FORM);
	await named(page, "jack");
	await page.getByLabel("Brief").fill("Follow me.");
	await page.getByRole("button", { name: "Open Workspace" }).click();

	await expect(page).toHaveURL(OPENED);
	const workspace = new URL(page.url()).pathname.split("/").at(-1) ?? "";
	await expect(page.getByRole("heading", { name: workspace })).toBeVisible();
	await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
	await expect(
		page.getByText("Queue order is unknown: no dispatch configuration is recorded.", {
			exact: true,
		}),
	).toBeVisible();
});

test("a draft survives opening a Workspace and coming back to the form", async ({ page }) => {
	await page.goto(FORM);
	await named(page, "jack");
	await page.getByLabel("Brief").fill("Keep this brief.");

	await page.getByRole("button", { name: "Open Workspace" }).click();
	await expect(page).toHaveURL(OPENED);

	await page.getByRole("link", { name: "New Workspace" }).click();
	await expect(page.getByLabel("Brief")).toHaveValue("Keep this brief.");
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");
	await expect(page.getByLabel("Agent")).toHaveValue("builder");
});

test("an existing Workspace in the pane is a link the form's draft outlives", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);

	await page.goto(FORM);
	await page.getByLabel("Brief").fill("Outlive me.");
	await page.getByRole("link", { name: workspace.name }).click();
	await expect(page).toHaveURL(OPENED);

	await page.getByRole("link", { name: "New Workspace" }).click();
	await expect(page.getByLabel("Brief")).toHaveValue("Outlive me.");
});

test("the form has no serious accessibility violation", async ({ page }) => {
	await page.goto(FORM);
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");

	const { violations } = await new AxeBuilder({ page }).analyze();

	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);
});
