import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

const ACTOR = "jack";

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

async function opened(request: APIRequestContext): Promise<string> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { name: string } };
	return body.workspace.name;
}

async function posting(request: APIRequestContext, workspace: string, message: string) {
	const response = await request.post(
		`/operator/organizations/acme/workspaces/${workspace}/messages`,
		{ data: { participant: ACTOR, message } },
	);
	expect(response.ok(), await response.text()).toBe(true);
}

// While `failing`, every queue read is cut before it is answered, as a lost connection is.
async function cuttingQueue(page: Page) {
	const reads = { failing: true };
	await page.route(
		(url) => url.pathname === "/operator/organizations/acme/queue",
		(route) => (reads.failing ? route.abort("connectionreset") : route.continue()),
	);
	return reads;
}

async function sessionsTab(page: Page, workspace: string) {
	await page.goto(`/organizations/acme/workspaces/${workspace}`);
	await expect(page.getByRole("heading", { name: workspace })).toBeVisible();
	const work = page.getByRole("region", { name: "Work", exact: true });
	await work.getByRole("tab", { name: "Sessions" }).click();
	return work.getByRole("region", { name: "Queued Sessions" });
}

test.describe("a queue read", () => {
	test("that fails names the error, stops reading, and recovers when read again", async ({
		page,
	}) => {
		const reads = await cuttingQueue(page);

		await page.goto("/organizations/acme/new");

		const refusal = page.getByRole("alert").filter({ hasText: "could not be reached" });
		await expect(refusal).toBeVisible({ timeout: 20_000 });
		await expect(page.getByText("Unknown until the queue is read.")).toBeVisible();
		await expect(page.getByText("Reading the queue…")).toHaveCount(0);
		await expect(
			page.getByRole("status").filter({ hasText: "The queue has not been read." }),
		).toBeVisible();

		reads.failing = false;
		const again = refusal.getByRole("button", { name: "Read again" });
		await again.focus();
		await expect(again).toBeFocused();
		await page.keyboard.press("Enter");

		await expect(refusal).toHaveCount(0);
		await expect(page.getByText("No dispatch configuration is recorded.")).toBeVisible();
	});

	test("that is slow says it is still reading, then shows the queue", async ({ page }) => {
		const held = Promise.withResolvers<void>();
		await page.route(
			(url) => url.pathname === "/operator/organizations/acme/queue",
			async (route) => {
				await held.promise;
				await route.continue();
			},
		);

		await page.goto("/organizations/acme/new");

		const reading = page.getByRole("status").filter({ hasText: "Still reading the queue" });
		await expect(reading.first()).toBeVisible({ timeout: 10_000 });

		held.resolve();

		await expect(reading).toHaveCount(0);
		await expect(page.getByText("No dispatch configuration is recorded.")).toBeVisible();
	});

	test("that fails after a read keeps the known queue and says when it was read", async ({
		page,
		request,
	}) => {
		const workspace = await opened(request);
		const queued = await sessionsTab(page, workspace);
		await expect(queued.getByRole("button", { name: /queued Sessions/ })).toBeVisible();
		await expect(queued.getByRole("listitem").first()).toBeVisible();

		const reads = await cuttingQueue(page);
		// Opening another Workspace changes the queue, and its notice refetches it.
		await opened(request);

		const refusal = queued.getByRole("alert");
		await expect(refusal).toContainText("could not be reached", { timeout: 20_000 });
		await expect(
			queued.getByRole("status").filter({ hasText: /Showing the queue as last read/ }),
		).toBeVisible();
		await expect(queued.getByRole("listitem").first()).toBeVisible();

		reads.failing = false;
		await refusal.getByRole("button", { name: "Read again" }).click();
		await expect(refusal).toHaveCount(0);
	});

	test("that finds nothing queued says so", async ({ page, request }) => {
		const workspace = await opened(request);
		await page.route(
			(url) => url.pathname === "/operator/organizations/acme/queue",
			(route) =>
				route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({
						work_role: null,
						active_work: { limit: null, occupied: 0, occupants: [], elsewhere: 0 },
						instances: { limit: null, count: 0, counted: [] },
						queued: [],
						waiting: [],
						unbriefed: [],
					}),
				}),
		);

		const queued = await sessionsTab(page, workspace);

		await expect(queued).toContainText("No Session is queued.");
		await expect(queued.getByRole("alert")).toHaveCount(0);
	});
});

test.describe("a Transcript follow", () => {
	test("that cannot reach the stream says the Transcript is unavailable, never empty", async ({
		page,
		request,
	}) => {
		const workspace = await opened(request);
		await posting(request, workspace, "one");
		const cut = { failing: true };
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			(route) => (cut.failing ? route.abort("connectionrefused") : route.continue()),
		);

		await page.goto(`/organizations/acme/workspaces/${workspace}`);

		const transcript = page.getByRole("region", { name: "Transcript" });
		const unavailable = transcript.getByRole("region", { name: "Transcript unavailable" });
		await expect(unavailable).toBeVisible({ timeout: 20_000 });
		await expect(unavailable.getByRole("alert")).toContainText("could not be reached");
		await expect(transcript.getByRole("heading", { name: "Transcript", exact: true })).toHaveCount(
			0,
		);

		cut.failing = false;
		const again = unavailable.getByRole("button", { name: "Read again" });
		await again.focus();
		await expect(again).toBeFocused();
		await page.keyboard.press("Enter");

		await expect(page.getByRole("log").getByText(`${ACTOR}: one`)).toBeVisible({
			timeout: 20_000,
		});
		await expect(unavailable).toHaveCount(0);
	});

	test("that drops keeps what it showed, says it is reconnecting, and resumes without duplicates", async ({
		page,
		request,
	}) => {
		const workspace = await opened(request);
		await posting(request, workspace, "one");

		const replay = await request.get(
			`/operator/organizations/acme/workspaces/${workspace}/transcript?follow=false`,
		);
		expect(replay.ok(), await replay.text()).toBe(true);
		const delivered = (await replay.text())
			.split("\n\n")
			.filter((block) => block.includes("event: entry"))
			.join("\n\n");

		const resumed = Promise.withResolvers<void>();
		let first = true;
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			async (route) => {
				if (first) {
					first = false;
					// Live once the follower registers, then the stream closes under it.
					await route.fulfill({
						status: 200,
						contentType: "text/event-stream",
						body: `${delivered}\n\nevent: follower\ndata: {"id":"6b1a1f2c-0000-0000-0000-000000000000","lease_seconds":60}\n\n`,
					});
					return;
				}
				await resumed.promise;
				await route.continue();
			},
		);

		await page.goto(`/organizations/acme/workspaces/${workspace}`);
		const log = page.getByRole("log");

		const reconnecting = page
			.getByRole("status")
			.filter({ hasText: "Reconnecting to the Transcript" });
		await expect(reconnecting).toBeVisible({ timeout: 20_000 });
		await expect(log.getByText(`${ACTOR}: one`)).toBeVisible();

		await posting(request, workspace, "two");
		resumed.resolve();

		await expect(log.getByText(`${ACTOR}: two`)).toBeVisible({ timeout: 20_000 });
		await expect(reconnecting).toHaveCount(0);
		await expect(log.getByText(`${ACTOR}: one`)).toHaveCount(1);
		await expect(log.getByText(`${ACTOR}: two`)).toHaveCount(1);
	});

	test("that reads an empty history says the Transcript is empty only once it is live", async ({
		page,
	}) => {
		const held = Promise.withResolvers<void>();
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			async (route) => {
				await held.promise;
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: 'event: follower\ndata: {"id":"6b1a1f2c-0000-0000-0000-000000000000","lease_seconds":60}\n\n',
				});
			},
		);
		const workspace = await opened(page.request);

		await page.goto(`/organizations/acme/workspaces/${workspace}`);

		const transcript = page.getByRole("region", { name: "Transcript" });
		await expect(
			transcript.getByRole("status").filter({ hasText: "Connecting to the Transcript…" }),
		).toBeVisible();
		await expect(transcript.getByRole("heading", { name: "Transcript", exact: true })).toHaveCount(
			0,
		);

		held.resolve();

		await expect(
			transcript.getByRole("heading", { name: "Transcript", exact: true }),
		).toBeVisible();
	});
});
