import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

// A separate install keeps the empty-install journey independent of other suites' declarations.
test("an empty install saves the Operator and resumes after reload", async ({
	page,
	playwright,
}) => {
	const directory = await mkdtemp(`${tmpdir()}/kestrel-setup-`);
	const url = "http://127.0.0.1:18718";
	const controlPlane = spawn(resolve("../../target/debug/kestrel-control-plane"), ["serve"], {
		env: {
			...process.env,
			KESTREL_DATA_DIR: directory,
			KESTREL_COMPUTE: "local-exec",
			KESTREL_LISTEN: "127.0.0.1:18717",
			KESTREL_OPERATOR_LISTEN: "127.0.0.1:18718",
		},
		stdio: "ignore",
	});
	const api = await playwright.request.newContext({ baseURL: url });
	try {
		await expect
			.poll(async () => {
				try {
					return (await api.get("/operator/install")).status();
				} catch {
					return 0;
				}
			})
			.toBe(200);
		await page.route(
			/\/operator\/(install|readiness|operator|organizations)(\?.*)?$/,
			async (route) => {
				const request = route.request();
				const response = await api.fetch(
					new URL(request.url()).pathname + new URL(request.url()).search,
					{
						method: request.method(),
						data: request.postData() ?? undefined,
						headers: { "content-type": "application/json" },
					},
				);
				await route.fulfill({ response });
			},
		);
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto("/");
		await expect(page.getByLabel("Your name")).toBeFocused();
		await expect(page.locator("[data-slot=questionnaire-progress]")).toContainText("1");
		const { violations } = await new AxeBuilder({ page }).analyze();
		expect(
			violations.filter(({ impact }) => impact === "serious" || impact === "critical"),
		).toEqual([]);
		expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
			true,
		);
		await page.screenshot({ path: "/tmp/kestrel-setup-empty.png" });
		await page.getByLabel("Your name").fill("river");
		await page.getByRole("button", { name: "Continue", exact: true }).click();
		await expect(page.getByRole("heading", { name: "Continue setup" })).toBeVisible();
		expect(await (await api.get("/operator/install")).json()).toEqual({
			initial_setup_complete: false,
		});
		expect(await (await api.get("/operator/organizations")).json()).toMatchObject([
			{ name: "river" },
		]);
		const identity = await (await api.get("/operator/operator")).json();
		await page.reload();
		await expect(page.getByRole("heading", { name: "Continue setup" })).toBeVisible();
		await expect(page.getByText("river", { exact: true })).toBeVisible();
		await page.getByRole("button", { name: "Back", exact: true }).click();
		await expect(page.getByLabel("Your name")).toHaveValue("river");
		await expect(page.getByLabel("Your name")).toBeFocused();
		await page.getByRole("button", { name: "Continue", exact: true }).click();
		expect(await (await api.get("/operator/operator")).json()).toEqual(identity);
		expect(await (await api.get("/operator/organizations")).json()).toHaveLength(1);
		await page.getByRole("button", { name: "Back", exact: true }).click();
		await page.getByLabel("Your name").fill("brook");
		await page.getByLabel("Your name").press("Enter");
		await expect(page.getByRole("heading", { name: "Continue setup" })).toBeVisible();
		expect(await (await api.get("/operator/operator")).json()).toEqual({
			...identity,
			name: "brook",
		});
		expect(await (await api.get("/operator/organizations")).json()).toMatchObject([
			{ name: "river" },
		]);
	} finally {
		controlPlane.kill();
		await api.dispose();
		await rm(directory, { recursive: true, force: true });
	}
});

const saved = {
	operator: { id: "00000000-0000-0000-0000-000000000001", name: "river" },
	sign_ins: [],
	github_integrations: [],
	projects: ["kestrel"],
};

const unavailable = {
	kind: "unavailable" as const,
	message: "The environment image could not be inspected",
	field: null,
	context: {
		service: "compute",
		resource: null,
		operation: "show_readiness",
		retry_after_seconds: null,
	},
	next_steps: [
		{
			action: "retry_read" as const,
			operation: "show_readiness",
			resource: null,
			retry_after_seconds: null,
		},
	],
};

test("completed setup with a later inspection failure offers repair without onboarding", async ({
	page,
	request,
}) => {
	await request.post("/operator/organizations", { data: { name: "acme" } });
	await page.route("**/operator/install", (route) =>
		route.fulfill({ json: { initial_setup_complete: true } }),
	);
	await page.route("**/operator/readiness*", (route) =>
		route.fulfill({
			json: {
				outcome: "unavailable",
				organization: "acme",
				client_url: "http://127.0.0.1:17719",
				initial_setup_complete: true,
				selected: {
					harness: null,
					model: null,
					profile: null,
					sign_in_method: null,
					project: null,
				},
				sign_in: null,
				saved,
				gaps: [],
				warnings: [],
				unavailable: [unavailable],
			},
		}),
	);
	await page.goto("/setup?organization=acme");
	await expect(page.getByRole("heading", { name: "Current readiness" })).toBeVisible();
	await expect(page.getByRole("status")).toHaveText("Readiness unavailable");
	await expect(page.getByLabel("Your name")).toHaveCount(0);
	await expect(page.getByText("The environment image could not be inspected")).toBeVisible();
	await expect(page.getByRole("button", { name: "Read again", exact: true })).toBeVisible();
	await expect(page.locator('[data-diagnostic="setup_gap"]')).toHaveCount(0);
	await expect(page.getByRole("link", { name: "Open Workspaces" })).toHaveAttribute(
		"href",
		"/organizations/acme",
	);
	await page.goto("/");
	await expect(page).toHaveURL(/\/organizations\/acme$/);
	await page.getByRole("link", { name: "Setup and readiness" }).click();
	await expect(page).toHaveURL(/\/setup\?organization=acme$/);
	await expect(page.getByRole("heading", { name: "Current readiness" })).toBeVisible();
});
