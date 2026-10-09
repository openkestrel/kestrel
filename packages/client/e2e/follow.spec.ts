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
	// Without a Brief, the first message would become the Brief rather than a Said entry.
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { name: string } };
	return body.workspace.name;
}

async function posting(
	request: APIRequestContext,
	workspace: string,
	message: string,
): Promise<void> {
	const response = await request.post(
		`/operator/organizations/acme/workspaces/${workspace}/messages`,
		{ data: { participant: ACTOR, message } },
	);
	expect(response.ok(), await response.text()).toBe(true);
}

// The tab's one stream, as opposed to the reservation and subscription requests beside it.
function streamsOpened(page: Page): string[] {
	const tokens: string[] = [];
	page.on("request", (request) => {
		const path = new URL(request.url()).pathname.split("/");
		if (request.method() === "GET" && path[2] === "streams" && path.length === 4) {
			tokens.push(path[3] ?? "");
		}
	});
	return tokens;
}

test("every tab hears a change through its own stream", async ({ context, request }) => {
	const first = await context.newPage();
	const second = await context.newPage();
	await first.goto("/organizations/acme");
	await second.goto("/organizations/acme");
	await expect(first.getByRole("link", { name: "New Workspace" })).toBeVisible();
	await expect(second.getByRole("link", { name: "New Workspace" })).toBeVisible();

	const name = await opened(request);

	await expect(first.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
	await expect(second.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
});

// HTTP/1.1 gives an origin six connections: five tabs hold one each and leave one for requests.
test("five tabs follow their Workspaces over HTTP/1.1, navigate, and still answer requests", async ({
	context,
	request,
}) => {
	const names = await Promise.all(Array.from({ length: 6 }, () => opened(request)));
	const followed = names.slice(0, 5);
	const elsewhere = names[5] ?? "";
	const pages = await Promise.all(followed.map(() => context.newPage()));
	const streams = pages.map(streamsOpened);
	for (const [index, page] of pages.entries()) {
		const name = followed[index] ?? "";
		// oxlint-disable-next-line no-await-in-loop -- each tab is open before the next, as an operator opens them.
		await page.goto(`/organizations/acme/workspaces/${name}`);
		// oxlint-disable-next-line no-await-in-loop -- as above.
		await expect(page.getByRole("heading", { name })).toBeVisible();
	}

	await Promise.all(followed.map((name) => posting(request, name, `to ${name}`)));
	for (const [index, page] of pages.entries()) {
		// oxlint-disable-next-line no-await-in-loop -- each tab is asserted in turn.
		await expect(page.getByRole("log").getByText(`${ACTOR}: to ${followed[index]}`)).toBeVisible({
			timeout: 20_000,
		});
	}

	const answered = await Promise.all(
		pages.map((page) => page.evaluate(async () => (await fetch("/operator/organizations")).status)),
	);
	expect(answered).toEqual([200, 200, 200, 200, 200]);

	const [moving] = pages;
	await moving
		.getByRole("navigation", { name: "Workspaces" })
		.getByRole("link", { name: elsewhere })
		.click();
	await expect(moving.getByRole("heading", { name: elsewhere })).toBeVisible();
	await posting(request, elsewhere, `to ${elsewhere}`);
	await expect(moving.getByRole("log").getByText(`${ACTOR}: to ${elsewhere}`)).toBeVisible({
		timeout: 20_000,
	});

	expect(streams.map((tokens) => tokens.length)).toEqual([1, 1, 1, 1, 1]);
});

test("a lost notice is healed by the refetch when the stream reconnects", async ({
	page,
	request,
}) => {
	let reservations = 0;
	let streams = 0;
	const held = Promise.withResolvers<void>();
	await page.route(
		(url) => url.pathname.startsWith("/operator/streams"),
		async (route) => {
			const path = new URL(route.request().url()).pathname.split("/");
			if (path.length === 3) {
				reservations += 1;
				if (reservations > 1) await held.promise;
			} else if (path.length === 4 && route.request().method() === "GET") {
				streams += 1;
				if (streams === 1) {
					await route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
					return;
				}
			}
			await route.continue();
		},
	);

	await page.goto("/organizations/acme");
	await expect(page.getByRole("link", { name: "New Workspace" })).toBeVisible();
	await expect.poll(() => reservations).toBe(2);

	const name = await opened(request);
	held.resolve();

	await expect(page.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
});

test("a dropped stream resumes the Transcript from its cursor without a gap or duplicate", async ({
	page,
	request,
}) => {
	const name = await opened(request);
	await posting(request, name, "one");

	const replay = await request.get(
		`/operator/organizations/acme/workspaces/${name}/transcript?follow=false`,
	);
	expect(replay.ok(), await replay.text()).toBe(true);
	const delivered = (await replay.text())
		.split("\n\n")
		.findLast((block) => block.includes("event: entry"));
	const lines = (delivered ?? "").split("\n");
	const cursor =
		lines
			.find((line) => line.startsWith("id:"))
			?.slice(3)
			.trim() ?? "";
	const data =
		lines
			.find((line) => line.startsWith("data:"))
			?.slice(5)
			.trim() ?? "";
	expect(cursor).not.toBe("");

	// The first connection carries "one" and closes; the reconnect is held until "two" exists,
	// so its arrival proves the new reservation resumed from the cursor.
	const followedAs = Promise.withResolvers<string>();
	const resumed = Promise.withResolvers<void>();
	const cut = Promise.withResolvers<void>();
	let streams = 0;
	await page.route(
		(url) => url.pathname.startsWith("/operator/streams/"),
		async (route) => {
			const call = route.request();
			const path = new URL(call.url()).pathname.split("/");
			if (call.method() === "PUT" && path.length === 6) {
				// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the Client's subscription.
				const body = call.postDataJSON() as { kind: string; after?: string };
				if (body.kind === "transcript") {
					followedAs.resolve(path[5] ?? "");
					if (body.after === cursor) resumed.resolve();
				}
			}
			if (call.method() === "GET" && path.length === 4) {
				streams += 1;
				if (streams === 1) {
					const subscription = await followedAs.promise;
					await route.fulfill({
						status: 200,
						contentType: "text/event-stream",
						body: `event: entry\ndata: {"subscription":${JSON.stringify(subscription)},"cursor":${JSON.stringify(cursor)},"data":${data}}\n\n`,
					});
					return;
				}
				await cut.promise;
			}
			await route.continue();
		},
	);

	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();
	await expect(page.getByRole("log").getByText(`${ACTOR}: one`)).toBeVisible({ timeout: 20_000 });

	await resumed.promise;
	await posting(request, name, "two");
	cut.resolve();

	await expect(page.getByRole("log").getByText(`${ACTOR}: two`)).toBeVisible({ timeout: 20_000 });
	await expect(page.getByRole("log").getByText(`${ACTOR}: one`)).toHaveCount(1);
	await expect(page.getByRole("log").getByText(`${ACTOR}: two`)).toHaveCount(1);
});
