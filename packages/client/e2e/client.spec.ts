import { AxeBuilder } from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test.beforeAll(async ({ request }) => {
	const declared = await request.post("/operator/organizations", { data: { name: "acme" } });
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

test("a refusal shows its typed context and links its inspection in the same Organization", async ({
	page,
}) => {
	await page.route("**/operator/organizations/acme/workspaces/brave-otter-abcdefgh", (route) =>
		route.fulfill({
			status: 409,
			contentType: "application/json",
			body: JSON.stringify({
				kind: "state_conflict",
				message: "the Workspace is sealed",
				field: null,
				context: {
					operation: "show_workspace",
					resource: "workspace",
					reference: "brave-otter-abcdefgh",
					organization: "acme",
					state: "sealed",
					holding_session: "calm-river",
				},
				next_steps: [{ action: "list_resources", resource: "workspace", organization: "acme" }],
			}),
		}),
	);

	await page.goto("/organizations/acme/workspaces/brave-otter-abcdefgh");

	const refusal = page.getByRole("region", { name: "Transcript" }).getByRole("alert");
	await expect(refusal).toContainText("the Workspace is sealed");
	await expect(refusal).toContainText("409");
	await expect(refusal).toContainText("sealed");
	await expect(refusal).toContainText("calm-river");

	await refusal.getByRole("link", { name: "List the Workspaces in acme" }).click();
	await expect(page).toHaveURL(/\/organizations\/acme$/);
});

test("a busy control plane's wait is kept before reading again", async ({ page }) => {
	let asked = 0;
	await page.route("**/operator/organizations", (route) => {
		asked += 1;
		return route.fulfill({
			status: 503,
			contentType: "application/json",
			headers: { "retry-after": "1" },
			body: JSON.stringify({
				kind: "unavailable",
				message: "the control plane could not answer",
				field: null,
				context: {
					service: "control_plane",
					resource: null,
					operation: "list_organizations",
					retry_after_seconds: 1,
				},
				next_steps: [
					{
						action: "retry_read",
						operation: "list_organizations",
						resource: null,
						retry_after_seconds: 1,
					},
				],
			}),
		});
	});

	await page.goto("/");

	const refusal = page.getByRole("alert");
	await expect(refusal).toContainText("the control plane could not answer", { timeout: 15_000 });
	await expect(refusal.getByRole("button", { name: /^Read again in \d s$/ })).toBeDisabled();
	const before = asked;
	await refusal.getByRole("button", { name: "Read again" }).click();
	await expect.poll(() => asked).toBeGreaterThan(before);
});

test("an answer that is not JSON still offers a read again and a connection check", async ({
	page,
}) => {
	await page.route("**/operator/organizations", (route) =>
		route.fulfill({ status: 404, contentType: "text/html", body: "<h1>Not Found</h1>" }),
	);

	await page.goto("/");

	const refusal = page.getByRole("alert");
	await expect(refusal).toContainText("the control plane answered 404");
	await expect(refusal.getByRole("button", { name: "Read again" })).toBeVisible();
	await expect(refusal).toContainText("is running and reachable from this browser");
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
