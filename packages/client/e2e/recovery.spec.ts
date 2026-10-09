import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { Streamed } from "./streamed";

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

// While `failing`, the control plane answers every queue read with a failure of its own; an
// unreachable one is the outage page's to explain.
async function failingQueue(page: Page) {
	const reads = { failing: true };
	await page.route(
		(url) => url.pathname === "/operator/organizations/acme/queue",
		(route) =>
			reads.failing
				? route.fulfill({ status: 500, json: { message: "the queue could not be computed" } })
				: route.continue(),
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
		const reads = await failingQueue(page);

		await page.goto("/organizations/acme/new");

		const refusal = page.getByRole("alert").filter({ hasText: "the queue could not be computed" });
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

		const reads = await failingQueue(page);
		// Opening another Workspace changes the queue, and its notice refetches it.
		await opened(request);

		const refusal = queued.getByRole("alert");
		await expect(refusal).toContainText("the queue could not be computed", { timeout: 20_000 });
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

// One subscription's events, framed as the tab's stream carries them.
function framed(subscription: string, events: { event: string; cursor?: string; data: unknown }[]) {
	return events
		.map(
			({ event, cursor, data }) =>
				`event: ${event}\ndata: ${JSON.stringify({ subscription, ...(cursor ? { cursor } : {}), data })}\n\n`,
		)
		.join("");
}

const FOLLOWER = { id: "6b1a1f2c-0000-0000-0000-000000000000", lease_seconds: 60 };

async function recordedEntries(request: APIRequestContext, workspace: string) {
	const replay = await request.get(
		`/operator/organizations/acme/workspaces/${workspace}/transcript?follow=false`,
	);
	expect(replay.ok(), await replay.text()).toBe(true);
	return (await replay.text())
		.split("\n\n")
		.filter((block) => block.includes("event: entry"))
		.map((block) => {
			const lines = block.split("\n");
			const field = (name: string) =>
				lines
					.find((line) => line.startsWith(`${name}:`))
					?.slice(name.length + 1)
					.trim() ?? "";
			return { event: "entry", cursor: field("id"), data: JSON.parse(field("data")) as unknown };
		});
}

test.describe("a Transcript follow", () => {
	test("that cannot reach the control plane says the Transcript is unavailable, never empty", async ({
		page,
		request,
	}) => {
		const workspace = await opened(request);
		await posting(request, workspace, "one");
		const cut = { failing: true };
		await page.route(
			(url) => url.pathname.startsWith("/operator/streams"),
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
		const reconnect = unavailable.getByRole("button", { name: "Reconnect" });
		await reconnect.focus();
		await expect(reconnect).toBeFocused();
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
		const entries = await recordedEntries(request, workspace);

		// The first connection carries "one" and a follower, then closes; the reconnect reaches the
		// control plane only once "two" exists.
		const followed = Promise.withResolvers<string>();
		const resumed = Promise.withResolvers<void>();
		let streams = 0;
		await page.route(
			(url) => url.pathname.startsWith("/operator/streams/"),
			async (route) => {
				const call = route.request();
				const path = new URL(call.url()).pathname.split("/");
				if (call.method() === "PUT" && path.length === 6) {
					// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the Client's subscription.
					const body = call.postDataJSON() as { kind: string };
					if (body.kind === "transcript") followed.resolve(path[5] ?? "");
				}
				if (call.method() === "GET" && path.length === 4) {
					streams += 1;
					if (streams === 1) {
						await route.fulfill({
							status: 200,
							contentType: "text/event-stream",
							body: framed(await followed.promise, [
								...entries,
								{ event: "follower", data: FOLLOWER },
							]),
						});
						return;
					}
					await resumed.promise;
				}
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
		request,
	}) => {
		const workspace = await opened(request);
		const held = Promise.withResolvers<void>();
		const streamed = new Streamed({
			transcript: async () => {
				await held.promise;
				return `event: follower\ndata: ${JSON.stringify(FOLLOWER)}\n\n`;
			},
		});
		await streamed.install(page);

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
