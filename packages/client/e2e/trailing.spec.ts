import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

const ORGANIZATION = "acme";
const REPOSITORY = "https://github.com/openkestrel/kestrel";
const SESSION = "00000000-0000-0000-0000-000000000101";

test.beforeAll(async ({ request }) => {
	await request.post("/operator/organizations", { data: { name: ORGANIZATION } });
	await request.post(`/operator/organizations/${ORGANIZATION}/projects`, {
		data: { name: "kestrel", repositories: [REPOSITORY], branch: "main" },
	});
	await request.post(`/operator/organizations/${ORGANIZATION}/agents`, {
		data: { name: "builder", harness: "opencode" },
	});
});

async function opened(request: APIRequestContext): Promise<string> {
	const response = await request.post(`/operator/organizations/${ORGANIZATION}/workspaces`, {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { name: string } };
	return body.workspace.name;
}

// The Workspace is real; only its work report and its Sessions are scripted.
class Reads {
	work: Record<string, unknown> = {
		state: "no_instance",
		branch: "kestrel/work",
		pull_request: null,
		last_report: { report: "none" },
		earlier_reports: [],
	};
	session: Record<string, unknown> = session();

	constructor(private readonly workspace: string) {}

	async install(page: Page): Promise<void> {
		const organization = `/operator/organizations/${ORGANIZATION}`;
		await page.route(
			(url) => url.pathname === `${organization}/workspaces/${this.workspace}/work`,
			(route) => route.fulfill({ json: this.work }),
		);
		await page.route(
			(url) => url.pathname === `${organization}/workspaces/${this.workspace}/sessions`,
			(route) => route.fulfill({ json: [this.session] }),
		);
		await page.route(
			(url) => url.pathname.startsWith(`${organization}/sessions/`),
			(route) =>
				route.request().method() === "GET"
					? route.fulfill({ json: this.session })
					: route.continue(),
		);
	}
}

function session(overrides: Record<string, unknown> = {}) {
	return {
		id: SESSION,
		name: "calm-river-abcdefg1",
		workspace: "00000000-0000-0000-0000-0000000000ff",
		state: "trailing",
		preparing: null,
		exit: null,
		outcome_message: null,
		diagnostic: null,
		instance: "local-2",
		supervisor: "supervisor-2",
		agent: "builder",
		harness: "opencode",
		model: null,
		mode: null,
		thought_level: null,
		worked_model: null,
		sign_in_method: null,
		supplied: [],
		title: null,
		options: [],
		changing_options: [],
		commands: [],
		interrupting: null,
		enqueued_at: "2026-09-30T10:00:00Z",
		started_at: "2026-09-30T10:00:05Z",
		ended_at: null,
		lease_expires_at: null,
		connected_at: "2026-09-30T10:00:04Z",
		supervisor_version: null,
		usage: null,
		tools: [],
		units: [],
		message_buffering: false,
		thought_buffering: false,
		last_activity_at: "2026-09-30T10:04:00Z",
		observation: { availability: "current", observed_at: "2026-09-30T10:04:00Z" },
		depends_on: [],
		...overrides,
	};
}

const BUILD = {
	call_id: "call-1",
	title: "cargo build",
	tool_kind: "execute",
	status: "in_progress",
	started_at: "2026-09-30T10:03:00Z",
};
const REVIEW = {
	id: "unit-1",
	kind: "subagent",
	title: "review the diff",
	started_at: "2026-09-30T10:03:10Z",
};
const SERVER = {
	id: "unit-2",
	kind: "background_task",
	title: "bun run dev",
	started_at: "2026-09-30T10:03:20Z",
};

function read(repository: string, overrides: Record<string, unknown> = {}) {
	return {
		repository,
		git: "read",
		branch: "kestrel/work",
		changed: { files: 4, added: 12, removed: 3 },
		staged: { files: 0, added: 0, removed: 0 },
		committed: { commits: 2, added: 8, removed: 2 },
		pushed: null,
		untracked: 1,
		stashed: 0,
		...overrides,
	};
}

function report(instance: string, overrides: Record<string, unknown> = {}) {
	return {
		report: "received",
		instance,
		current_instance: false,
		reported_at: "2026-09-30T10:02:00Z",
		repositories: [read(REPOSITORY)],
		...overrides,
	};
}

async function viewing(page: Page, workspace: string): Promise<void> {
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace}`);
	await expect(page.getByRole("heading", { name: workspace })).toBeVisible();
}

function workPane(page: Page) {
	return page.getByRole("region", { name: "Work", exact: true });
}

function trailing(page: Page) {
	return page.getByRole("region", { name: "Trailing work" });
}

test("a trailing Session names its open tools and Adapter Units, and still takes a Turn", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	reads.session = session({ tools: [BUILD], units: [REVIEW, SERVER] });
	await reads.install(page);
	await viewing(page, workspace);

	await expect(trailing(page).getByRole("status")).toHaveText(
		"The agent answered, and work it started is still open. You can post a new Turn now.",
	);
	await expect(
		trailing(page).getByRole("list", { name: "Open work" }).getByRole("listitem"),
	).toHaveText(["cargo buildtool", "review the diffsubagent", "bun run devbackground task"]);
	await expect(page.getByRole("textbox", { name: "Post" })).toBeEditable();
	await expect(page.getByRole("button", { name: "Post", exact: true })).toBeVisible();
});

test("a trailing Session observed with nothing open only checks that work has settled", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	await reads.install(page);
	await viewing(page, workspace);

	await expect(trailing(page).getByRole("status")).toHaveText(
		"Checking that background work has settled. You can post a new Turn now.",
	);
	// Neither a countdown nor a time it will finish by.
	await expect(trailing(page)).not.toContainText(/\d/);
	await expect(trailing(page).getByRole("list")).toHaveCount(0);
	await expect(page.getByRole("textbox", { name: "Post" })).toBeEditable();
});

test("an unobserved trailing Session shows its last observation as history, never a quiet check", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	reads.session = session({
		observation: {
			availability: "unavailable",
			last: { observed_at: "2026-09-30T10:04:00Z", tools: [BUILD], units: [SERVER] },
		},
	});
	await reads.install(page);
	await viewing(page, workspace);

	await expect(trailing(page).getByRole("status")).toHaveText(
		"Open work can’t be observed right now. You can post a new Turn now.",
	);
	await expect(trailing(page).getByRole("time")).toHaveAttribute(
		"datetime",
		"2026-09-30T10:04:00Z",
	);
	await expect(trailing(page)).toContainText("This is history, not what is open now.");
	await expect(
		trailing(page)
			.getByRole("list", { name: "Work open when last observed" })
			.getByRole("listitem"),
	).toHaveText(["cargo buildtool", "bun run devbackground task"]);
	await expect(trailing(page)).not.toContainText("settled");

	// A restarted control plane holds no snapshot, and a fresh browser has nothing of its own to show.
	reads.session = session({ observation: { availability: "unavailable", last: null } });
	await page.reload();
	await expect(trailing(page).getByRole("status")).toContainText(
		"Open work can’t be observed right now.",
	);
	await expect(trailing(page).getByRole("list")).toHaveCount(0);
	await expect(trailing(page)).not.toContainText("settled");
});

test("a stopped Instance's last report stays as history beside the unavailable reading", async ({
	page,
	browser,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	reads.work = {
		state: "no_instance",
		branch: "kestrel/work",
		pull_request: null,
		last_report: report("local-2", {
			repositories: [
				read(REPOSITORY),
				{
					repository: "https://github.com/openkestrel/docs",
					git: "unreadable",
					because: "the checkout has no HEAD",
				},
			],
		}),
		earlier_reports: [
			report("local-1", {
				reported_at: "2026-09-29T10:00:00Z",
				repositories: [read(REPOSITORY, { branch: "kestrel/first" })],
			}),
		],
	};
	await reads.install(page);
	await viewing(page, workspace);
	const work = workPane(page);

	await expect(work.getByText("No Instance is running")).toBeVisible();
	const last = work.getByRole("button", {
		name: /^Last reported .+ by the earlier Instance local-2$/,
	});
	await expect(last).toHaveAttribute("aria-expanded", "true");
	await expect(work.getByText("This is history, not the work as it is now.")).toBeVisible();
	await expect(work.getByRole("time").first()).toHaveAttribute("datetime", "2026-09-30T10:02:00Z");
	await expect(work.getByText("2 commits +8 −2")).toBeVisible();
	await expect(work.getByText("the checkout has no HEAD")).toBeVisible();

	const earlier = work.getByRole("button", {
		name: /^Last reported .+ by the earlier Instance local-1$/,
	});
	await expect(earlier).toHaveAttribute("aria-expanded", "false");
	await expect(work.getByText("kestrel/first")).toBeHidden();
	await earlier.focus();
	await page.keyboard.press("Enter");
	await expect(earlier).toBeFocused();
	await expect(earlier).toHaveAttribute("aria-expanded", "true");
	await expect(work.getByText("kestrel/first")).toBeVisible();

	// The report is the control plane's, so a browser that never saw it reads the same history.
	const fresh = await browser.newContext();
	const second = await fresh.newPage();
	await reads.install(second);
	await viewing(second, workspace);
	await expect(
		workPane(second).getByRole("button", {
			name: /^Last reported .+ by the earlier Instance local-2$/,
		}),
	).toHaveAttribute("aria-expanded", "true");
	await expect(workPane(second).getByText("the checkout has no HEAD")).toBeVisible();
	await fresh.close();
});

test("a Workspace no Instance has reported for says so", async ({ page, request }) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	reads.work = {
		state: "not_answering",
		message: "the Instance isn't answering",
		last_report: { report: "none" },
		earlier_reports: [],
	};
	await reads.install(page);
	await viewing(page, workspace);
	const work = workPane(page);

	await expect(work.getByText("the Instance isn't answering")).toBeVisible();
	await expect(work.getByText("No work report received.")).toBeVisible();
	await expect(work.getByRole("button", { name: /^Last reported/ })).toHaveCount(0);
});

test("a replaced Instance's report stays its own under the replacement's live reading", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	reads.work = {
		state: "reported",
		reported_at: new Date().toISOString(),
		repositories: [read(REPOSITORY, { branch: "kestrel/replacement" })],
		last_report: report("local-2", { current_instance: true }),
		earlier_reports: [
			report("local-1", { repositories: [read(REPOSITORY, { branch: "kestrel/replaced" })] }),
		],
	};
	await reads.install(page);
	await viewing(page, workspace);
	const work = workPane(page);

	await expect(work.getByText("reported just now")).toBeVisible();
	await expect(work.getByText("kestrel/replacement")).toBeVisible();
	const history = work.getByRole("button", { name: /^Last reported/ });
	await expect(history).toHaveCount(1);
	await expect(history).toHaveAccessibleName(/by the earlier Instance local-1$/);
	await expect(history).toHaveAttribute("aria-expanded", "false");
	await expect(work.getByText("kestrel/replaced")).toBeHidden();

	await history.click();
	await expect(work.getByText("kestrel/replaced")).toBeVisible();
	await expect(work.getByText("This is history, not the work as it is now.")).toBeVisible();
});

test("a lease is a deadline: passing it ends nothing here, and the control plane's extension shows", async ({
	page,
	request,
}) => {
	test.setTimeout(90_000);
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	const deadline = new Date(Date.now() + 8_000).toISOString();
	reads.session = session({ state: "working", lease_expires_at: deadline });
	await reads.install(page);
	await viewing(page, workspace);
	const work = workPane(page);
	await work.getByRole("tab", { name: "Sessions" }).click();
	const card = work.getByRole("article").filter({ hasText: "calm-river-abcdefg1" });
	const lease = card.locator("[data-session-lease]");

	await expect(lease).toContainText("Lease expires in less than a minute");
	await expect(lease.getByRole("time")).toHaveAttribute("datetime", deadline);
	await expect(card).not.toContainText(/lease just now/i);

	await expect(lease).toContainText("Lease expired less than a minute ago", { timeout: 20_000 });
	await expect(card.getByText("Working")).toBeVisible();
	await expect(card.getByText("Ended")).toHaveCount(0);

	// What ADR-0054's grace does: the control plane moves the deadline, and only its answer changes the view.
	const extended = new Date(Date.now() + 10 * 60_000).toISOString();
	reads.session = session({ state: "working", lease_expires_at: extended });
	await expect(lease).toContainText("Lease expires in 10 minutes", { timeout: 30_000 });
	await expect(lease.getByRole("time")).toHaveAttribute("datetime", extended);

	reads.session = session({
		state: "ended",
		ended_at: new Date().toISOString(),
		exit: { status: "failed", because: "its lease expired" },
	});
	await page.reload();
	await work.getByRole("tab", { name: "Sessions" }).click();
	await expect(card.getByText("failed: its lease expired")).toBeVisible();
	await expect(card.locator("[data-session-lease]")).toHaveCount(0);
});

test("a failed Session's stack opens as a stack, and its prose evidence stays prose", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reads = new Reads(workspace);
	const failed = (summary: string) =>
		session({
			state: "ended",
			exit: { status: "failed", because: "the agent stopped" },
			diagnostic: {
				kind: "unknown_failure",
				message: "the agent stopped without saying why",
				field: null,
				context: { session: "s-1", resource: null, evidence: { kind: "unknown", summary } },
				next_steps: [
					{
						action: "inspect_resource",
						resource: "session",
						reference: "s-1",
						organization: ORGANIZATION,
					},
				],
			},
		});
	reads.session = failed(
		"TypeError: session is undefined\n    at render (src/app.js:10:5)\n    at src/main.js:3:1",
	);
	await reads.install(page);
	await viewing(page, workspace);
	const work = workPane(page);
	await work.getByRole("tab", { name: "Sessions" }).click();
	const why = work.getByRole("region", { name: "Why it failed" });

	const stack = why.getByRole("button", { name: /TypeError\s*session is undefined/ });
	await expect(stack).toHaveAttribute("aria-expanded", "false");
	await stack.focus();
	await page.keyboard.press("Enter");
	await expect(stack).toHaveAttribute("aria-expanded", "true");
	await expect(why.getByText("src/app.js:10:5")).toBeVisible();
	await expect(why.getByText("src/main.js:3:1")).toBeVisible();
	await expect(
		why.getByText(`Inspect the Session s-1 in ${ORGANIZATION}`, { exact: false }),
	).toBeVisible();
	await expect(work.getByRole("alert")).toHaveCount(0);

	// A backtrace Stack Trace cannot draw as frames is shown whole, so no line of it is lost.
	reads.session = failed(
		"thread 'main' panicked at src/main.rs:4:5:\nboom\nstack backtrace:\n   0: rust_begin_unwind\n             at /rustc/abc/library/std/src/panicking.rs:652:5",
	);
	await page.reload();
	await work.getByRole("tab", { name: "Sessions" }).click();
	const backtrace = why.getByRole("button", { name: /panicked at src\/main\.rs:4:5/ });
	await backtrace.click();
	await expect(backtrace).toHaveAttribute("aria-expanded", "true");
	await expect(why.getByText("0: rust_begin_unwind")).toBeVisible();
	await expect(why.getByText("boom")).toBeVisible();

	reads.session = failed("the model refused the request");
	await page.reload();
	await work.getByRole("tab", { name: "Sessions" }).click();
	await expect(why.getByText("the model refused the request")).toBeVisible();
	await expect(why.getByRole("button", { name: /refused/ })).toHaveCount(0);
});

test("People says who has joined and who is following now", async ({ page, request }) => {
	const workspace = await opened(request);
	await viewing(page, workspace);
	const work = workPane(page);
	await work.getByRole("tab", { name: "People" }).click();
	const people = work.getByRole("tabpanel", { name: "People" });

	await expect(people.getByRole("heading", { name: "Joined" })).toBeVisible();
	await expect(people.getByText("Everyone who has taken part in this Workspace.")).toBeVisible();
	await expect(people.getByRole("list", { name: "Joined" }).getByRole("listitem")).toHaveText([
		"builder",
	]);
	await expect(people.getByRole("heading", { name: "Following now" })).toBeVisible();
	await expect(people.getByText("Everyone with this Workspace open right now.")).toBeVisible();
	await expect(
		people.getByRole("list", { name: "Following now" }).getByRole("listitem"),
	).toHaveText(["1 anonymous"]);
	await expect(people).not.toContainText(/durable|transient|Transcript/);
});
