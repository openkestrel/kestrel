import { type ChildProcess, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

const AGENTS = "/operator/organizations/acme/agents";
const SETTINGS = "/organizations/acme/settings/agents";

// Its own install: the other suites lean on acme holding one Agent, and an Agent is never undeclared.
let controlPlane: ChildProcess;
let api: APIRequestContext;
let directory: string;

test.beforeAll(async ({ playwright, request }) => {
	directory = await mkdtemp(`${tmpdir()}/kestrel-agents-`);
	controlPlane = spawn(resolve("../../target/debug/kestrel-control-plane"), ["serve"], {
		env: {
			...process.env,
			KESTREL_DATA_DIR: directory,
			KESTREL_COMPUTE: "local-exec",
			KESTREL_LISTEN: "127.0.0.1:18727",
			KESTREL_OPERATOR_LISTEN: "127.0.0.1:18728",
		},
		stdio: "ignore",
	});
	api = await playwright.request.newContext({ baseURL: "http://127.0.0.1:18728" });
	await expect
		.poll(async () => {
			try {
				return (await api.get("/operator/install")).status();
			} catch {
				return 0;
			}
		})
		.toBe(200);
	await api.post("/operator/organizations", { data: { name: "acme" } });
	// The tab's change notices still subscribe on the shared control plane.
	await request.post("/operator/organizations", { data: { name: "acme" } });
});

test.afterAll(async () => {
	controlPlane.kill();
	await api.dispose();
	await rm(directory, { recursive: true, force: true });
});

test.beforeEach(async ({ page }) => {
	await page.route(
		(url) => url.pathname.startsWith("/operator/") && !url.pathname.startsWith("/operator/streams"),
		async (route) => {
			const request = route.request();
			const url = new URL(request.url());
			const response = await api.fetch(url.pathname + url.search, {
				method: request.method(),
				data: request.postData() ?? undefined,
				headers: { "content-type": "application/json" },
			});
			await route.fulfill({ response });
		},
	);
});

async function declared(name: string): Promise<unknown> {
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's Agent list.
	const agents = (await (await api.get(AGENTS)).json()) as { name: string }[];
	return agents.find((agent) => agent.name === name);
}

function unavailable(operation: string) {
	return {
		kind: "unavailable",
		message: "The store is busy",
		field: null,
		context: { service: "store", resource: null, operation, retry_after_seconds: null },
		next_steps: [{ action: "retry_read", operation, resource: null, retry_after_seconds: null }],
	};
}

function form(page: Page) {
	return page.locator("[data-agent-form]");
}

test("an Agent is declared on the harness's default and edited to a model id, keeping its other defaults", async ({
	page,
}) => {
	await api.post(AGENTS, {
		data: { name: "settled", harness: "claude", mode: "plan", thought_level: "high" },
	});
	await page.goto(SETTINGS);
	await expect(page.getByRole("heading", { name: "Agents", level: 1 })).toBeVisible();
	await expect(page.locator('[data-agent="settled"]')).toContainText(
		"claude · model the harness's default · mode plan · thought level high",
	);

	await page.getByRole("button", { name: "Declare an Agent" }).click();
	await form(page).getByLabel("Name").fill("drafter");
	await expect(form(page).getByLabel("Harness").locator("option")).toHaveText([
		"Choose a harness",
		"opencode",
		"claude",
		"codex",
	]);
	await form(page).getByLabel("Harness").selectOption("codex");
	await expect(form(page).locator("[data-harness-command]")).toContainText("codex-acp");
	await expect(form(page).locator("[data-model-line]")).toHaveText("Uses the harness's default.");

	const { violations } = await new AxeBuilder({ page }).analyze();
	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);

	await form(page).getByRole("button", { name: "Save Agent" }).click();
	await expect(page.getByRole("status")).toHaveText("Saved the Agent drafter.");
	await expect(page.locator('[data-agent="drafter"]')).toContainText(
		"codex · model the harness's default",
	);
	expect(await declared("drafter")).toMatchObject({
		harness: "codex",
		model: null,
		mode: null,
		thought_level: null,
	});

	await page.getByRole("button", { name: "Edit settled" }).click();
	await expect(form(page).getByLabel("Name")).toHaveValue("settled");
	await expect(form(page).getByLabel("Name")).toHaveAttribute("readonly");
	await form(page).getByLabel("Model").fill("claude-opus-4-5");
	await expect(form(page).locator("[data-model-line]")).toContainText(
		"Overrides the harness's default with claude-opus-4-5, taken at its word",
	);
	await form(page).getByRole("button", { name: "Save Agent" }).click();
	await expect(page.locator('[data-agent="settled"]')).toContainText("model claude-opus-4-5");
	expect(await declared("settled")).toMatchObject({
		harness: "claude",
		model: "claude-opus-4-5",
		mode: "plan",
		thought_level: "high",
	});

	await page.getByRole("button", { name: "Edit settled" }).click();
	await form(page).getByRole("button", { name: "Use the harness's default" }).click();
	await expect(form(page).getByLabel("Model")).toHaveValue("");
	await form(page).getByRole("button", { name: "Save Agent" }).click();
	await expect(page.locator('[data-agent="settled"]')).toContainText("model the harness's default");
	expect(await declared("settled")).toMatchObject({ model: null, mode: "plan" });
});

test("a slow then failed Agents read says so, keeps what was entered and reads again", async ({
	page,
}) => {
	await api.post(AGENTS, { data: { name: "listed", harness: "opencode" } });
	const reads = { failing: true, slow: true };
	await page.route(
		(url) => url.pathname === AGENTS,
		async (route) => {
			if (route.request().method() !== "GET" || !reads.failing) return route.fallback();
			if (reads.slow) await new Promise((waited) => setTimeout(waited, 2_500));
			reads.slow = false;
			return route.fulfill({ status: 503, json: unavailable("list_agents") });
		},
	);
	await page.goto(SETTINGS);

	await expect(page.locator('[data-agents-read="delayed"]')).toHaveText(
		"Still reading the Agents; the Operator has not answered yet.",
	);
	const failed = page.locator('[data-agents-read="failed"]');
	// The read is tried again once before it is reported.
	await expect(failed).toContainText("The store is busy", { timeout: 15_000 });
	await expect(page.locator("[data-agents-read=empty]")).toHaveCount(0);

	await page.getByRole("button", { name: "Declare an Agent" }).click();
	await form(page).getByLabel("Name").fill("kept");
	await form(page).getByLabel("Harness").selectOption("claude");
	await form(page).getByLabel("Model").fill("opus");

	reads.failing = false;
	await failed.getByRole("button", { name: "Read again" }).click();
	await expect(page.locator('[data-agent="listed"]')).toBeVisible();
	await expect(failed).toHaveCount(0);
	await expect(form(page).getByLabel("Name")).toHaveValue("kept");
	await expect(form(page).getByLabel("Harness")).toHaveValue("claude");
	await expect(form(page).getByLabel("Model")).toHaveValue("opus");
});

test("a refused save stands at its field, keeps every input and saves on the next try", async ({
	page,
}) => {
	const writes = { refusing: true };
	await page.route(
		(url) => url.pathname === AGENTS,
		(route) =>
			route.request().method() === "POST" && writes.refusing
				? route.fulfill({
						status: 422,
						json: {
							kind: "invalid_field",
							message: "An Agent's name holds no spaces",
							field: "name",
							context: { operation: "declare_agent", resource: "agent", organization: "acme" },
							next_steps: [
								{
									action: "correct_field",
									field: "name",
									constraint: "no_spaces",
									allowed_values: null,
								},
							],
						},
					})
				: route.fallback(),
	);
	await page.goto(SETTINGS);
	await page.getByRole("button", { name: "Declare an Agent" }).click();
	await form(page).getByRole("button", { name: "Save Agent" }).click();
	await expect(form(page).getByRole("alert")).toHaveText(["Name the Agent.", "Choose a harness."]);

	await form(page).getByLabel("Name").fill("retried");
	await form(page).getByLabel("Harness").selectOption("opencode");
	await form(page).getByLabel("Model").fill("anthropic/claude-sonnet-4-5");
	await form(page).getByLabel("Mode", { exact: true }).fill("build");
	await form(page).getByRole("button", { name: "Save Agent" }).click();

	await expect(form(page).getByRole("alert")).toHaveText("An Agent's name holds no spaces");
	await expect(form(page).getByLabel("Name")).toHaveAttribute("aria-invalid", "true");
	await expect(form(page).getByLabel("Model")).toHaveValue("anthropic/claude-sonnet-4-5");
	await expect(form(page).getByLabel("Mode", { exact: true })).toHaveValue("build");
	expect(await declared("retried")).toBeUndefined();

	writes.refusing = false;
	await form(page).getByRole("button", { name: "Save Agent" }).click();
	await expect(page.locator('[data-agent="retried"]')).toContainText(
		"opencode · model anthropic/claude-sonnet-4-5 · mode build",
	);
});

test("no declared Agent reads as empty, and the image decides which harness may be chosen", async ({
	page,
}) => {
	await page.route(
		(url) => url.pathname === AGENTS,
		(route) =>
			route.request().method() === "GET" ? route.fulfill({ json: [] }) : route.fallback(),
	);
	const image = { state: "not_carried", image: "kestrel-env:main", identity: "sha256:1" };
	const uninspected = {
		kind: "unavailable",
		message: "The image kestrel-env:main could not be inspected",
		field: null,
		context: {
			service: "compute",
			resource: null,
			operation: "list_harnesses",
			retry_after_seconds: null,
		},
		next_steps: [],
	};
	await page.route("**/operator/harnesses", (route) =>
		route.fulfill({
			json: [
				{
					name: "opencode",
					command: "opencode acp --print-logs",
					sign_in_methods: [],
					availability: { ...image, state: "available", diagnostic: null },
				},
				{
					name: "claude",
					command: "claude-agent-acp",
					sign_in_methods: [],
					availability: { ...image, diagnostic: null },
				},
				{
					name: "codex",
					command: "codex-acp",
					sign_in_methods: [],
					availability: { ...image, state: "unavailable", identity: null, diagnostic: uninspected },
				},
			],
		}),
	);
	await page.goto(SETTINGS);
	await expect(page.locator('[data-agents-read="empty"]')).toContainText(
		"No Agent is declared yet.",
	);

	await page.getByRole("button", { name: "Declare an Agent" }).click();
	const harness = form(page).getByLabel("Harness");
	await expect(harness.locator("option")).toHaveText([
		"Choose a harness",
		"opencode",
		"claude (not in the image)",
		"codex (image not inspected)",
	]);
	await expect(harness.locator("option", { hasText: "claude" })).toBeDisabled();
	await harness.selectOption("codex");
	await expect(form(page).locator('[data-diagnostic="unavailable"]')).toContainText(
		"The image kestrel-env:main could not be inspected",
	);
});

test("a failed catalogue read keeps the harness an Agent declares and reads again", async ({
	page,
}) => {
	await api.post(AGENTS, { data: { name: "catalogued", harness: "codex" } });
	const reads = { failing: true };
	await page.route("**/operator/harnesses", (route) =>
		reads.failing
			? route.fulfill({ status: 503, json: unavailable("list_harnesses") })
			: route.fallback(),
	);
	await page.goto(SETTINGS);
	await page.getByRole("button", { name: "Edit catalogued" }).click();

	const failed = form(page).locator('[data-harnesses-read="failed"]');
	await expect(failed).toContainText("The store is busy");
	await expect(form(page).getByLabel("Harness")).toHaveValue("codex");
	await form(page).getByLabel("Thought level").fill("high");

	reads.failing = false;
	await failed.getByRole("button", { name: "Read again" }).click();
	await expect(form(page).locator("[data-harness-command]")).toContainText("codex-acp");
	await expect(form(page).getByLabel("Harness")).toHaveValue("codex");
	await expect(form(page).getByLabel("Thought level")).toHaveValue("high");
});

test("the new Workspace form sends an empty Agent list to Agents settings", async ({ page }) => {
	await page.route(
		(url) => url.pathname === AGENTS,
		(route) => route.fulfill({ json: [] }),
	);
	await page.goto("/organizations/acme/new");
	await page.locator("[data-no-agents]").getByRole("link").click();
	await expect(page).toHaveURL(new RegExp(`${SETTINGS}$`));
});
