import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test.beforeAll(async ({ request }) => {
	const declared = await request.post("/operator/organizations", {
		data: { name: "acme" },
		headers: { "X-Kestrel-Operator": "1" },
	});
	expect(declared.ok(), await declared.text()).toBe(true);
});

test("a deep link survives a refresh and shows the control plane's refusal", async ({ page }) => {
	await page.goto("/organizations/acme/workspaces/brave-otter-abcdefgh");

	const transcript = page.getByRole("region", { name: "Transcript" });
	await expect(transcript.getByRole("alert")).toContainText("brave-otter-abcdefgh");
	await expect(transcript.getByRole("alert")).toContainText("404");

	await page.reload();

	await expect(transcript.getByRole("alert")).toContainText("brave-otter-abcdefgh");
	await expect(page.getByRole("heading", { name: "Workspaces" })).toBeVisible();
	await expect(page.getByRole("heading", { name: "Work", exact: true })).toBeVisible();
});

test("a refusal shows the field and the phase it names", async ({ page }) => {
	await page.route("**/operator/organizations/acme/workspaces/brave-otter-abcdefgh", (route) =>
		route.fulfill({
			status: 409,
			contentType: "application/json",
			body: JSON.stringify({
				message: "the Session is working",
				field: "value",
				phase: "working",
			}),
		}),
	);

	await page.goto("/organizations/acme/workspaces/brave-otter-abcdefgh");

	const refusal = page.getByRole("region", { name: "Transcript" }).getByRole("alert");
	await expect(refusal).toContainText("the Session is working");
	await expect(refusal).toContainText("409");
	await expect(refusal.locator("code", { hasText: "value" })).toBeVisible();
	await expect(refusal).toContainText("working");
});

test("a missing asset is not found, where a Client route is the page", async ({ request }) => {
	const missing = await request.get("/assets/nothing-here.js");
	expect(missing.status()).toBe(404);

	const route = await request.get("/organizations/acme/new");
	expect(route.status()).toBe(200);
	expect(await route.text()).toContain("<title>kestrel</title>");
});

test("the only Organization opens without being chosen", async ({ page }) => {
	await page.goto("/");

	await expect(page).toHaveURL(/\/organizations\/acme$/);
	await expect(page.getByRole("link", { name: "New Workspace" })).toBeVisible();
});

test("the workbench has no serious accessibility violation", async ({ page }) => {
	await page.goto("/organizations/acme/workspaces/brave-otter-abcdefgh");
	await expect(page.getByRole("alert").first()).toBeVisible();

	const { violations } = await new AxeBuilder({ page }).analyze();

	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);
});
