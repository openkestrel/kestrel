import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

const ORGANIZATION = "acme";

test.beforeAll(async ({ request }) => {
	await request.post("/operator/organizations", { data: { name: ORGANIZATION } });
	await request.post(`/operator/organizations/${ORGANIZATION}/projects`, {
		data: {
			name: "kestrel",
			repositories: ["https://github.com/openkestrel/kestrel"],
			branch: "main",
		},
	});
	await request.post(`/operator/organizations/${ORGANIZATION}/agents`, {
		data: { name: "builder", harness: "opencode" },
	});
});

async function opened(
	request: APIRequestContext,
	brief: string | undefined,
): Promise<{ id: string; name: string }> {
	const response = await request.post(`/operator/organizations/${ORGANIZATION}/workspaces`, {
		data: { project: "kestrel", agent: "builder", brief },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return body.workspace;
}

function id(index: number): string {
	return `00000000-0000-0000-0000-0000000000${String(index).padStart(2, "0")}`;
}

function workspace(index: number, overrides: Record<string, unknown> = {}) {
	const name = `row-${index}`;
	return {
		id: id(index),
		name,
		organization: ORGANIZATION,
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: {
			repositories: ["https://github.com/openkestrel/kestrel"],
			base: "main",
			branch: `kestrel/${name}`,
		},
		instance: null,
		held: null,
		held_messages: [],
		correlation: null,
		state: "open",
		opened_at: "2026-09-30T10:00:00Z",
		last_active_at: "2026-09-30T10:00:00Z",
		sealed_at: null,
		continues: null,
		started_by: null,
		continued_by: [],
		pull_requests: [],
		unfinished_session: null,
		...overrides,
	};
}

function session(index: number, overrides: Record<string, unknown> = {}) {
	return {
		id: id(100 + index),
		name: `calm-river-abcdefg${index}`,
		workspace: id(index),
		state: "queued",
		preparing: null,
		exit: null,
		outcome_message: null,
		instance: null,
		supervisor: null,
		agent: "builder",
		harness: "opencode",
		model: null,
		mode: null,
		thought_level: null,
		worked_model: null,
		title: null,
		options: [],
		commands: [],
		enqueued_at: "2026-09-30T10:00:00Z",
		started_at: null,
		ended_at: null,
		lease_expires_at: null,
		connected_at: null,
		supervisor_version: null,
		usage: null,
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

function queue(overrides: Record<string, unknown> = {}) {
	return {
		work_role: null,
		active_work: { limit: null, occupied: 0, occupants: [], elsewhere: 0 },
		instances: { limit: null, count: 0, counted: [] },
		queued: [],
		waiting: [],
		unbriefed: [],
		...overrides,
	};
}

function queued(index: number, overrides: Record<string, unknown> = {}) {
	return {
		position: 1,
		name: `calm-river-abcdefg${index}`,
		workspace: id(index),
		agent: "builder",
		reasons: [],
		enqueued_at: "2026-09-30T10:00:00Z",
		...overrides,
	};
}

class Reads {
	workspaces: unknown[] = [];
	queue: unknown = queue();
	sessions = new Map<number, unknown[]>();
	work = new Map<number, unknown>();

	async install(page: Page): Promise<void> {
		await page.route(
			(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/workspaces`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.workspaces),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/queue`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.queue),
				});
			},
		);
		await page.route(
			(url) => url.pathname.endsWith("/sessions"),
			async (route) => {
				const name = new URL(route.request().url()).pathname.split("/").at(-2) ?? "";
				const index = Number(name.replace("row-", ""));
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.sessions.get(index) ?? []),
				});
			},
		);
		await page.route(
			(url) => url.pathname.endsWith("/work"),
			async (route) => {
				const name = new URL(route.request().url()).pathname.split("/").at(-2) ?? "";
				const index = Number(name.replace("row-", ""));
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(
						this.work.get(index) ?? { state: "no_instance", branch: "main", pull_request: null },
					),
				});
			},
		);
	}
}

function noticing(page: Page) {
	const { promise: sent, resolve: send } = Promise.withResolvers<string>();
	let connections = 0;
	void page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/changes`,
		async (route) => {
			connections += 1;
			const body = connections === 1 ? "event: open\ndata: {}\n\n" : await sent;
			await route.fulfill({ status: 200, contentType: "text/event-stream", body });
		},
	);
	return {
		change: (changed: unknown) => send(`event: change\ndata: ${JSON.stringify(changed)}\n\n`),
	};
}

function rows(page: Page) {
	return page.locator('nav[aria-label="Workspaces"] li a');
}

function learnedPull(state: string) {
	return {
		repository: "https://github.com/openkestrel/kestrel",
		number: 7,
		url: "https://github.com/openkestrel/kestrel/pull/7",
		title: "Keep the list current",
		state,
		head_branch: "kestrel/row-21",
		head_revision: "abcdef",
		updated_at: "2026-09-30T10:00:00Z",
		event: id(90),
	};
}

test("a ready queued Session shows its FIFO place, and notices add the next one", async ({
	page,
	request,
}) => {
	const first = await opened(request, "do the first thing");
	await page.goto(`/organizations/${ORGANIZATION}`);

	await expect(page.locator("[data-queue-header]")).toContainText("Slots 0 (no limit)");
	await expect(page.locator("[data-queue-header]")).toContainText("Instances 0 (no limit)");

	// Other specs share this Organization, so the exact place is asserted on a scripted queue below.
	const queuedRow = rows(page).filter({ hasText: first.name });
	await expect(queuedRow).toContainText(/Queued #\d+/);
	await expect(queuedRow).toContainText("kestrel/");
	await expect(queuedRow).toContainText("no Instance");

	const second = await opened(request, "do the second thing");
	await expect(rows(page).filter({ hasText: second.name })).toContainText(/Queued #\d+/);
	await expect(rows(page).filter({ hasText: first.name })).toContainText(/Queued #\d+/);
});

test("attention outranks working, waiting and queued", async ({ page }) => {
	const reads = new Reads();
	reads.workspaces = [
		workspace(4),
		workspace(2),
		workspace(1, { held: "unpublished work" }),
		workspace(3),
	];
	reads.sessions = new Map([
		[1, [session(1, { state: "ended", exit: { status: "failed", because: "it broke" } })]],
		[2, [session(2, { state: "working", title: "a working turn" })]],
		[3, [session(3, { state: "waiting" })]],
		[4, [session(4, { state: "queued" })]],
	]);
	reads.queue = queue({ queued: [queued(4, { position: 1 })] });
	await reads.install(page);

	await page.goto(`/organizations/${ORGANIZATION}`);
	await expect(rows(page)).toHaveCount(4);

	const order = await rows(page).allTextContents();
	expect(order[0]).toContain("row-1");
	expect(order[1]).toContain("row-2");
	expect(order[2]).toContain("row-3");
	expect(order[3]).toContain("row-4");

	await expect(rows(page).nth(0)).toHaveAttribute("data-attention", "true");
	await expect(rows(page).nth(1)).toHaveAttribute("data-attention", "false");
	await expect(rows(page).nth(0)).toContainText("Attention");
	await expect(rows(page).nth(1)).toContainText("Working");
	await expect(rows(page).nth(1)).toContainText("a working turn");
	await expect(rows(page).nth(2)).toContainText("Waiting");
	await expect(rows(page).nth(3)).toContainText("Queued #1");
});

test("a dependency wait and an Instance wait show the queue's reasons, not an estimate", async ({
	page,
}) => {
	const reads = new Reads();
	reads.workspaces = [workspace(11), workspace(12), workspace(13)];
	reads.sessions = new Map([
		[11, [session(11)]],
		[12, [session(12)]],
		[13, [session(13, { state: "waiting" })]],
	]);
	reads.queue = queue({
		queued: [
			queued(11, {
				position: null,
				reasons: [{ kind: "dependencies", sessions: ["calm-river-abcdefg99"] }],
			}),
			queued(12, { position: null, reasons: [{ kind: "live_instance_limit", limit: 2 }] }),
		],
		waiting: [
			{
				name: "calm-river-abcdefg13",
				workspace: id(13),
				agent: "builder",
				pending_since: "2026-09-30T10:01:00Z",
				reasons: [{ kind: "active_work_slots", limit: 1 }],
				enqueued_at: "2026-09-30T10:00:00Z",
			},
		],
	});
	await reads.install(page);

	await page.goto(`/organizations/${ORGANIZATION}`);
	await expect(rows(page)).toHaveCount(3);

	await expect(rows(page).nth(0)).toContainText("Waiting");
	await expect(rows(page).nth(0)).toContainText("every Active-Work Slot is occupied (1)");
	await expect(rows(page).nth(1)).toContainText("waiting on calm-river-abcdefg99");
	await expect(rows(page).nth(1)).toContainText("Queued");
	await expect(rows(page).nth(2)).toContainText("at the live Instance limit of 2");

	await Promise.all([1, 2].map((row) => expect(rows(page).nth(row)).not.toContainText("Queued #")));
});

test("changed work and a learned pull request update on a Workspace notice", async ({ page }) => {
	const reads = new Reads();
	reads.workspaces = [
		workspace(21, {
			pull_requests: [
				{
					repository: "https://github.com/openkestrel/kestrel",
					availability: "available",
					known: [learnedPull("open")],
				},
			],
		}),
	];
	reads.sessions = new Map([[21, [session(21, { state: "working", title: "the turn" })]]]);
	const notice = noticing(page);
	reads.work = new Map([
		[
			21,
			{
				state: "reported",
				reported_at: new Date().toISOString(),
				repositories: [
					{
						repository: "https://github.com/openkestrel/kestrel",
						git: "read",
						branch: "kestrel/row-21",
						changed: { files: 2, added: 10, removed: 3 },
						staged: { files: 1, added: 4, removed: 1 },
						committed: { commits: 0, added: 0, removed: 0 },
						pushed: null,
						untracked: 2,
						stashed: 0,
					},
				],
			},
		],
	]);
	await reads.install(page);

	await page.goto(`/organizations/${ORGANIZATION}`);
	const row = rows(page).filter({ hasText: "row-21" });
	await expect(row).toContainText("+14 −4");
	await expect(row).toContainText("2 untracked");
	await expect(row).toContainText("#7 open");
	await expect(row).toContainText("reported just now");

	reads.workspaces = [
		workspace(21, {
			pull_requests: [
				{
					repository: "https://github.com/openkestrel/kestrel",
					availability: "available",
					known: [learnedPull("merged")],
				},
			],
		}),
	];
	reads.work = new Map([
		[
			21,
			{
				state: "reported",
				reported_at: new Date().toISOString(),
				repositories: [
					{
						repository: "https://github.com/openkestrel/kestrel",
						git: "read",
						branch: "kestrel/row-21",
						changed: { files: 0, added: 0, removed: 0 },
						staged: { files: 0, added: 0, removed: 0 },
						committed: { commits: 1, added: 5, removed: 1 },
						pushed: "abcdef",
						untracked: 0,
						stashed: 0,
					},
				],
			},
		],
	]);

	notice.change({ resource: "workspace", id: id(21) });

	await expect(row).toContainText("#7 merged");
	await expect(row).toContainText("+0 −0");
});

test("an unbriefed Session shows its preparing step and the header its driver", async ({
	page,
}) => {
	const reads = new Reads();
	reads.workspaces = [workspace(31)];
	reads.sessions = new Map([
		[31, [session(31, { state: "unbriefed", preparing: "cloning", title: null })]],
	]);
	reads.queue = queue({
		work_role: { active_work_slots: 2, serialized_harnesses: [], driver: "local" },
		active_work: { limit: 2, occupied: 0, occupants: [], elsewhere: 0 },
		instances: { limit: 1, count: 1, counted: ["local-1"] },
		unbriefed: [
			{
				name: "calm-river-abcdefg31",
				workspace: id(31),
				agent: "builder",
				preparing: "cloning",
				pending_since: null,
				enqueued_at: "2026-09-30T10:00:00Z",
			},
		],
	});
	await reads.install(page);

	await page.goto(`/organizations/${ORGANIZATION}`);
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).nth(0)).toContainText("Preparing");
	await expect(rows(page).nth(0)).toContainText("cloning");
	await expect(page.locator("[data-queue-header]")).toContainText("Slots 0/2");
	await expect(page.locator("[data-queue-header]")).toContainText("Instances 1/1");
	await expect(page.locator("[data-queue-header]")).toContainText("local");
});
