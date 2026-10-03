import { expect, test, type APIRequestContext } from "@playwright/test";

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

test("every tab opens its own notice stream, and a change reaches each", async ({
	context,
	request,
}) => {
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

// Twelve event streams on one origin, which HTTP/1.1's six connections could not carry.
test("six tabs each follow their own Workspace at once", async ({ context, request }) => {
	const names = await Promise.all(Array.from({ length: 6 }, () => opened(request)));
	const pages = await Promise.all(names.map(() => context.newPage()));
	for (const [index, page] of pages.entries()) {
		const name = names[index] ?? "";
		// oxlint-disable-next-line no-await-in-loop -- each tab is open before the next, as an operator opens them.
		await page.goto(`/organizations/acme/workspaces/${name}`);
		// oxlint-disable-next-line no-await-in-loop -- as above.
		await expect(page.getByRole("heading", { name })).toBeVisible();
	}

	await Promise.all(names.map((name) => posting(request, name, `to ${name}`)));

	for (const [index, page] of pages.entries()) {
		// oxlint-disable-next-line no-await-in-loop -- each tab is asserted in turn.
		await expect(page.getByRole("log").getByText(`${ACTOR}: to ${names[index]}`)).toBeVisible({
			timeout: 20_000,
		});
	}
});

test("a lost notice is healed by the refetch when the stream reconnects", async ({
	page,
	request,
}) => {
	let first = true;
	const held = Promise.withResolvers<void>();
	await page.route("**/changes", async (route) => {
		if (first) {
			first = false;
			await route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: "event: open\ndata: {}\n\n",
			});
			return;
		}
		await held.promise;
		await route.continue();
	});

	await page.goto("/organizations/acme");
	await expect(page.getByRole("link", { name: "New Workspace" })).toBeVisible();

	const name = await opened(request);
	held.resolve();

	await expect(page.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
});

test("a follow reconnect resumes from its cursor without a gap or duplicate", async ({
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
	expect(delivered).toContain("data:");

	// The reconnect can be in flight before the entry is visible, so the route captures it.
	const cut = Promise.withResolvers<void>();
	const resumed = Promise.withResolvers<void>();
	let first = true;
	await page.route(
		(url) => url.pathname.endsWith("/transcript"),
		async (route) => {
			if (first) {
				first = false;
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: `${delivered}\n\n`,
				});
				return;
			}
			if ((route.request().headers()["last-event-id"] ?? "") !== "") resumed.resolve();
			await cut.promise;
			await route.continue();
		},
	);

	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();
	await expect(page.getByRole("log").getByText(`${ACTOR}: one`)).toBeVisible({ timeout: 20_000 });

	// Held until "two" exists, so its arrival proves the reconnect resumed from the cursor.
	await resumed.promise;
	await posting(request, name, "two");
	cut.resolve();

	await expect(page.getByRole("log").getByText(`${ACTOR}: two`)).toBeVisible({ timeout: 20_000 });
	await expect(page.getByRole("log").getByText(`${ACTOR}: one`)).toHaveCount(1);
	await expect(page.getByRole("log").getByText(`${ACTOR}: two`)).toHaveCount(1);
});
