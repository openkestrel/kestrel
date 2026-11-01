import { expect, test, type Page, type Route } from "@playwright/test";

// Registering a GitHub Integration asks GitHub, so its record and maintenance writes are stubbed.
const ORGANIZATION = "upkeep";
const INTEGRATIONS = `/operator/organizations/${ORGANIZATION}/integrations`;
const SETTINGS = `/organizations/${ORGANIZATION}/settings/repositories`;

type Held = { integration: Record<string, unknown>; writes: { path: string; body: unknown }[] };
type Answer = (route: Route, held: Held) => Promise<void> | undefined;

function github(patch: Record<string, unknown> = {}) {
	return {
		id: "00000000-0000-0000-0000-0000000000aa",
		name: "github",
		kind: "github",
		repository: "upkeep/repo",
		bot_login: "kestrel-example[bot]",
		carries: ["inbound", "outbound"],
		state: "enabled",
		revision: 3,
		disabled_at: null,
		retired_at: null,
		canceled_posts: [],
		polled_every: "1m",
		webhook_path: null,
		last_event_refusal: null,
		diagnostic: null,
		webhook_secret_rotation: null,
		...patch,
	};
}

const project = {
	id: "00000000-0000-0000-0000-0000000000bb",
	name: "repo",
	repositories: ["https://github.com/upkeep/repo.git"],
	integrations: [
		{
			repository: "https://github.com/upkeep/repo.git",
			integration: "github",
			integration_id: "00000000-0000-0000-0000-0000000000aa",
		},
	],
	branch: "main",
};

const RESUME = {
	action: "resume_integration_webhook_secret_rotation",
	organization: ORGANIZATION,
	integration: "github",
	retry_after_seconds: null,
};

function unacknowledged(operation: string) {
	return {
		kind: "unavailable",
		message: "GitHub did not acknowledge the new webhook secret",
		field: null,
		context: {
			service: "github",
			resource: "integration",
			operation,
			retry_after_seconds: null,
			saved: null,
		},
		next_steps: [RESUME],
	};
}

function conflict(operation: string, message: string, steps: unknown[]) {
	return {
		kind: "state_conflict",
		message,
		field: null,
		context: {
			operation,
			resource: "integration",
			reference: "github",
			organization: ORGANIZATION,
			state: "disabled",
			holding_session: null,
		},
		next_steps: [
			{
				action: "inspect_resource",
				resource: "integration",
				reference: "github",
				organization: ORGANIZATION,
			},
			...steps,
		],
	};
}

const DISABLED = conflict(
	"show_integration",
	"the integration github is disabled; enable it to resume what waits on it",
	[{ action: "enable_integration", organization: ORGANIZATION, integration: "github" }],
);

test.beforeAll(async ({ request }) => {
	await request.post("/operator/organizations", { data: { name: ORGANIZATION } });
});

async function stub(page: Page, held: Held, answers: Partial<Record<string, Answer>> = {}) {
	await page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/projects`,
		(route) =>
			route.request().method() === "GET" ? route.fulfill({ json: [project] }) : route.fallback(),
	);
	await page.route(
		(url) => url.pathname.startsWith(INTEGRATIONS),
		async (route) => {
			const request = route.request();
			const rest = new URL(request.url()).pathname.slice(INTEGRATIONS.length);
			if (request.method() === "GET") {
				if (rest === "") return route.fulfill({ json: [held.integration] });
				if (rest === "/github/webhook")
					return route.fulfill({
						json: { url: "https://hooks.example/github", pending_url: null, deliveries: [] },
					});
				return route.fulfill({ json: held.integration });
			}
			held.writes.push({ path: rest, body: request.postDataJSON() });
			const answer = answers[rest];
			if (answer) return answer(route, held);
			return route.fulfill({ status: 404, json: {} });
		},
	);
}

function card(page: Page) {
	return page.locator('[data-integration="github"]');
}

function settle(held: Held, patch: Record<string, unknown>): Answer {
	return (route) => {
		held.integration = { ...held.integration, ...patch };
		return route.fulfill({ json: held.integration });
	};
}

test("pausing shows what waits before it is confirmed, and resuming carries on", async ({
	page,
}) => {
	const held: Held = { integration: github(), writes: [] };
	await stub(page, held, {
		"/github/disable": (route, h) =>
			settle(h, {
				state: "disabled",
				disabled_at: new Date().toISOString(),
				revision: 4,
				diagnostic: DISABLED,
			})(route, h),
		"/github/enable": (route, h) =>
			settle(h, { state: "enabled", disabled_at: null, revision: 5, diagnostic: null })(route, h),
	});
	await page.goto(SETTINGS);
	await expect(card(page).locator("[data-lifecycle-state]")).toHaveText("In use");
	await expect(card(page).locator("[data-lifecycle]")).toContainText(
		"Carries inbound and outbound for upkeep/repo in upkeep",
	);

	await card(page).getByRole("button", { name: "Pause…" }).click();
	await expect(card(page).locator('[data-confirm="pause"] [data-consequence]')).toContainText(
		"Its key, secret, poll cursor and pending posts are kept",
	);
	expect(held.writes).toEqual([]);
	await card(page).getByRole("button", { name: "Pause github" }).click();

	await expect(card(page).locator("[data-lifecycle-state]")).toHaveText("Paused");
	await expect(card(page).locator("[data-lifecycle]")).toContainText("kept for when it resumes");
	await expect(page.locator('[data-project="repo"]')).toContainText(
		"reaches GitHub as github, which is paused",
	);

	await expect(card(page).getByRole("button", { name: "Pause…" })).toHaveCount(0);
	await card(page).getByRole("button", { name: "Enable the Integration github" }).click();
	await expect(card(page).locator("[data-lifecycle-state]")).toHaveText("In use");
	expect(held.writes.map(({ path }) => path)).toEqual(["/github/disable", "/github/enable"]);
});

test("a rejected key keeps the current one, and a shared App's hook is the producer's to refuse", async ({
	page,
}) => {
	const held: Held = { integration: github(), writes: [] };
	await stub(page, held, {
		"/github/private-key": (route) =>
			route.fulfill({
				status: 422,
				json: {
					kind: "invalid_field",
					message: "GitHub does not accept this key for the App",
					field: "private_key",
					context: { field: "private_key", constraint: "accepted_by_github", allowed_values: null },
					next_steps: [
						{
							action: "replace_integration_private_key",
							organization: ORGANIZATION,
							integration: "github",
							retry_after_seconds: null,
						},
					],
				},
			}),
		"/github/webhook-secret-rotation": (route) =>
			route.fulfill({
				status: 403,
				json: {
					kind: "forbidden_action",
					message:
						"another integration that is not retired signs as the same App as github, perhaps in another organization; the App's webhook is theirs too, so it is not changed from here",
					field: null,
					context: {
						operation: "rotate_integration_webhook_secret",
						resource: "integration",
						reference: "github",
						organization: ORGANIZATION,
						constraint: "shared_app",
					},
					next_steps: [
						{
							action: "inspect_resource",
							resource: "integration",
							reference: "github",
							organization: ORGANIZATION,
						},
					],
				},
			}),
	});
	await page.goto(SETTINGS);
	await card(page).getByRole("button", { name: "Replace the App private key…" }).click();
	const form = card(page).locator("[data-key-replacement]");
	await expect(form.locator("[data-consequence]")).toContainText("reach upkeep/repo");
	const key = form.getByLabel("Replacement private key");
	await expect(key).toHaveAttribute("type", "file");
	await key.setInputFiles({
		name: "app.pem",
		mimeType: "application/x-pem-file",
		buffer: Buffer.from(
			"-----BEGIN RSA PRIVATE KEY-----\nnot-really\n-----END RSA PRIVATE KEY-----\n",
		),
	});
	await form.getByRole("button", { name: "Replace the key" }).click();

	await expect(form.getByRole("alert")).toHaveText("GitHub does not accept this key for the App");
	await expect(form.locator("[data-key-kept]")).toHaveText(
		"Nothing was saved; the current key stays in use.",
	);
	await expect(page.getByText("not-really")).toHaveCount(0);
	await expect(card(page).locator("[data-lifecycle-state]")).toHaveText("In use");
	expect(held.writes).toEqual([
		{
			path: "/github/private-key",
			body: {
				private_key: "-----BEGIN RSA PRIVATE KEY-----\nnot-really\n-----END RSA PRIVATE KEY-----\n",
				revision: 3,
			},
		},
	]);

	await form.getByRole("button", { name: "Cancel" }).click();
	await card(page).getByRole("button", { name: "Rotate the webhook secret…" }).click();
	await card(page).getByRole("button", { name: "Rotate the webhook secret github" }).click();
	await expect(card(page).locator('[data-diagnostic="forbidden_action"]')).toContainText(
		"signs as the same App as github",
	);
	expect(held.writes.map(({ path }) => path)).toEqual([
		"/github/private-key",
		"/github/webhook-secret-rotation",
	]);
});

test("an interrupted rotation is resumed from the producer's own step, never resent by itself", async ({
	page,
}) => {
	const held: Held = { integration: github(), writes: [] };
	await stub(page, held, {
		"/github/webhook-secret-rotation": (route, h) => {
			if (h.integration.webhook_secret_rotation)
				return route.fulfill({
					status: 409,
					json: conflict(
						"rotate_integration_webhook_secret",
						"the integration github is already rotating its webhook secret",
						[RESUME],
					),
				});
			h.integration = {
				...h.integration,
				webhook_secret_rotation: {
					started_at: new Date().toISOString(),
					attempted_at: new Date().toISOString(),
					diagnostic: unacknowledged("rotate_integration_webhook_secret"),
				},
			};
			return route.fulfill({
				status: 503,
				json: unacknowledged("rotate_integration_webhook_secret"),
			});
		},
		"/github/webhook-secret-rotation/resume": (route, h) =>
			settle(h, { webhook_secret_rotation: null })(route, h),
	});
	await page.goto(SETTINGS);
	await card(page).getByRole("button", { name: "Rotate the webhook secret…" }).click();
	await expect(card(page).locator('[data-confirm="rotate"] [data-consequence]')).toContainText(
		"can be resumed here",
	);
	await card(page).getByRole("button", { name: "Rotate the webhook secret github" }).click();
	await expect(
		card(page).locator("[data-maintenance] [data-diagnostic=unavailable]"),
	).toContainText("GitHub did not acknowledge");

	await page.reload();
	const rotation = card(page).locator("[data-rotation]");
	await expect(rotation).toContainText("is unfinished");
	expect(held.writes.map(({ path }) => path)).toEqual(["/github/webhook-secret-rotation"]);

	await card(page).getByRole("button", { name: "Rotate the webhook secret…" }).click();
	await card(page).getByRole("button", { name: "Rotate the webhook secret github" }).click();
	await expect(
		card(page).locator("[data-maintenance] [data-diagnostic=state_conflict]"),
	).toContainText("already rotating");

	await rotation
		.getByRole("button", { name: "Resume the Integration github's webhook-secret rotation" })
		.click();
	await expect(card(page).locator("[data-rotation]")).toHaveCount(0);
	expect(held.writes.map(({ path }) => path)).toEqual([
		"/github/webhook-secret-rotation",
		"/github/webhook-secret-rotation",
		"/github/webhook-secret-rotation/resume",
	]);
});

test("retiring is confirmed by name after its consequences, and leaves readable history", async ({
	page,
}) => {
	const held: Held = { integration: github(), writes: [] };
	await stub(page, held, {
		"/github/retire": (route, h) =>
			settle(h, {
				state: "retired",
				retired_at: new Date().toISOString(),
				revision: 4,
				canceled_posts: [
					{
						session: "00000000-0000-0000-0000-0000000000cc",
						turn: null,
						canceled_at: new Date().toISOString(),
						reason: "its Integration was retired",
						attempted_at: null,
					},
				],
			})(route, h),
	});
	await page.goto(SETTINGS);
	await card(page).getByRole("button", { name: "Retire…" }).click();
	const confirm = card(page).locator('[data-confirm="retire"]');
	await expect(confirm.locator("[data-consequence]")).toContainText("Retiring is permanent.");
	await expect(confirm.locator("[data-consequence]")).toContainText("Nothing changes on GitHub");
	const retire = confirm.getByRole("button", { name: "Retire github" });
	await expect(retire).toBeDisabled();
	await confirm.getByLabel("Type github to retire it").fill("githu");
	await expect(retire).toBeDisabled();
	await confirm.getByLabel("Type github to retire it").fill("github");
	await retire.click();

	await expect(card(page).locator("[data-lifecycle-state]")).toHaveText("Retired");
	await expect(card(page).locator("[data-lifecycle]")).toContainText("untouched on GitHub");
	await expect(card(page).locator("[data-canceled-post]")).toContainText("'s Outcome, canceled");
	await expect(card(page).locator("[data-canceled-post]")).toContainText(
		"its Integration was retired",
	);
	await expect(
		card(page).getByRole("button", { name: /Pause|Resume|Retire|Replace|Rotate/ }),
	).toHaveCount(0);
	await expect(card(page).locator("[data-webhook]")).toHaveCount(0);
	await expect(page.locator('[data-project="repo"]')).toContainText(
		"reaches GitHub as github, which is retired",
	);
	expect(held.writes).toEqual([{ path: "/github/retire", body: { revision: 3 } }]);
});
