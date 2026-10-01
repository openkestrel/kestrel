import {
	expect,
	test,
	type APIRequestContext,
	type BrowserContext,
	type Page,
	type Request,
} from "@playwright/test";

declare global {
	interface Window {
		kestrelHidden: (hidden: boolean) => void;
	}
}

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
	// Opened with a Brief so a later message is recorded as a Said entry: the first message to an
	// unbriefed Workspace becomes its Brief instead (0.3/17).
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

function isStream(url: string): boolean {
	return url.includes("/changes") || url.includes("/transcript");
}

// The SSE connections each page has open now, and every one it ever opened.
class Streams {
	private readonly pages: Page[] = [];
	private readonly seen = new Map<Page, Request[]>();
	private readonly live = new Map<Page, Set<Request>>();

	watch(page: Page): void {
		const seen: Request[] = [];
		const live = new Set<Request>();
		this.pages.push(page);
		this.seen.set(page, seen);
		this.live.set(page, live);
		page.on("request", (request) => {
			if (!isStream(request.url())) return;
			seen.push(request);
			live.add(request);
		});
		const finished = (request: Request) => live.delete(request);
		page.on("requestfinished", finished);
		page.on("requestfailed", finished);
	}

	forget(page: Page): void {
		this.seen.delete(page);
		this.live.delete(page);
	}

	liveWith(needle: string): Request[] {
		return this.pages
			.flatMap((page) => [...(this.live.get(page) ?? [])])
			.filter((request) => request.url().includes(needle));
	}

	seenWith(needle: string): number {
		return [...this.seen.values()].reduce(
			(count, requests) =>
				count + requests.filter((request) => request.url().includes(needle)).length,
			0,
		);
	}

	everFor(page: Page, needle: string): boolean {
		return (this.seen.get(page) ?? []).some((request) => request.url().includes(needle));
	}
}

async function viewing(context: BrowserContext, streams: Streams): Promise<Page> {
	const page = await context.newPage();
	await page.addInitScript(() => {
		Object.defineProperty(document, "visibilityState", {
			configurable: true,
			get: () => "visible",
		});
	});
	streams.watch(page);
	return page;
}

async function visiting(page: Page, name: string): Promise<void> {
	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();
}

test("two tabs share one notice stream, and a change reaches both", async ({
	context,
	request,
}) => {
	const streams = new Streams();
	const first = await context.newPage();
	const second = await context.newPage();
	streams.watch(first);
	streams.watch(second);

	await first.goto("/organizations/acme");
	await second.goto("/organizations/acme");
	await expect(first.getByRole("link", { name: "New Workspace" })).toBeVisible();

	await expect.poll(() => streams.liveWith("/changes").length, { timeout: 20_000 }).toBe(1);

	const name = await opened(request);

	await expect(first.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
	await expect(second.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
});

test("the remaining tab takes the notice stream over when the leader closes", async ({
	context,
	request,
}) => {
	const streams = new Streams();
	const first = await context.newPage();
	const second = await context.newPage();
	streams.watch(first);
	streams.watch(second);

	await first.goto("/organizations/acme");
	await second.goto("/organizations/acme");
	await expect(second.getByRole("link", { name: "New Workspace" })).toBeVisible();
	await expect.poll(() => streams.liveWith("/changes").length, { timeout: 20_000 }).toBe(1);

	const takeover = second.waitForRequest((candidate) => candidate.url().includes("/changes"), {
		timeout: 20_000,
	});
	streams.forget(first);
	await first.close();
	await takeover;

	const name = await opened(request);

	await expect(second.getByRole("link", { name })).toBeVisible({ timeout: 20_000 });
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

test("a hidden tab holds no follow, and resumes from its cursor when visible again", async ({
	page,
	request,
}) => {
	await page.addInitScript(() => {
		let hidden = false;
		Object.defineProperty(document, "visibilityState", {
			configurable: true,
			get: () => (hidden ? "hidden" : "visible"),
		});
		window.kestrelHidden = (value) => {
			hidden = value;
			document.dispatchEvent(new Event("visibilitychange"));
		};
	});
	const name = await opened(request);
	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();

	await posting(request, name, "before");
	await expect(page.getByText(`${ACTOR}: before`)).toBeVisible({ timeout: 20_000 });

	await page.evaluate(() => window.kestrelHidden(true));
	await page.waitForTimeout(500);

	await posting(request, name, "while hidden");
	await expect(page.getByText(`${ACTOR}: while hidden`)).toHaveCount(0);

	await page.evaluate(() => window.kestrelHidden(false));

	await expect(page.getByText(`${ACTOR}: while hidden`)).toBeVisible({ timeout: 20_000 });
});

test("more than four visible views keep at most four SSE connections", async ({
	context,
	request,
}) => {
	const streams = new Streams();
	const pages = await Promise.all(Array.from({ length: 5 }, () => viewing(context, streams)));
	const names = await Promise.all(pages.map(() => opened(request)));

	for (const [index, page] of pages.entries()) {
		// oxlint-disable-next-line no-await-in-loop -- each view's wish is made in order, which is what ranks it.
		await visiting(page, names[index] ?? "");
	}

	await expect.poll(() => streams.liveWith("follow=true").length, { timeout: 20_000 }).toBe(3);
	await expect.poll(() => streams.liveWith("/changes").length, { timeout: 20_000 }).toBe(1);

	for (const [index, page] of pages.entries()) {
		const followed = streams
			.liveWith("follow=true")
			.find((candidate) => candidate.frame().page() === page);
		if (!followed) continue;
		expect(followed.url()).toContain(encodeURIComponent(names[index] ?? ""));
	}

	await expect.poll(() => streams.seenWith("follow=false"), { timeout: 20_000 }).toBeGreaterThan(0);

	const leaving = pages[0];
	if (!leaving) throw new Error("no view");
	streams.forget(leaving);
	await leaving.close();

	await expect.poll(() => streams.liveWith("follow=true").length, { timeout: 20_000 }).toBe(3);
	await expect.poll(() => streams.liveWith("/changes").length, { timeout: 20_000 }).toBe(1);
	await expect
		.poll(() => pages.slice(3).some((page) => streams.everFor(page, "follow=true")), {
			timeout: 20_000,
		})
		.toBe(true);
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

	const cut = Promise.withResolvers<void>();
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
			await cut.promise;
			await route.continue();
		},
	);

	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();
	await expect(page.getByText(`${ACTOR}: one`)).toBeVisible({ timeout: 20_000 });

	const resumed = page.waitForRequest(
		(candidate) =>
			candidate.url().includes("/transcript") &&
			(candidate.headers()["last-event-id"] ?? "") !== "",
		{ timeout: 20_000 },
	);
	await posting(request, name, "two");
	cut.resolve();

	await expect(page.getByText(`${ACTOR}: two`)).toBeVisible({ timeout: 20_000 });
	await resumed;
	await expect(page.getByText(`${ACTOR}: one`)).toHaveCount(1);
	await expect(page.getByText(`${ACTOR}: two`)).toHaveCount(1);
});
