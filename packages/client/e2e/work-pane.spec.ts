import {
	expect,
	test,
	type APIRequestContext,
	type BrowserContext,
	type Page,
} from "@playwright/test";

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

function workPane(page: Page) {
	return page.getByRole("region", { name: "Work", exact: true });
}

async function identified(context: BrowserContext, name: string): Promise<void> {
	const remembered: [string, string] = ["kestrel:participant", name];
	await context.addInitScript(([key, value]: [string, string]) => {
		localStorage.setItem(key, value);
	}, remembered);
}

async function viewing(page: Page, workspace: string): Promise<void> {
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace}`);
	await expect(page.getByRole("heading", { name: workspace })).toBeVisible();
}

test("a Workspace with no Instance says so, and shows its Session and joined Participants", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	await viewing(page, workspace.name);
	const work = workPane(page);

	await expect(work.getByText(`declared branch kestrel/${workspace.id}`)).toBeVisible();
	await expect(work.getByText("pull requests unavailable")).toBeVisible();
	await expect(work.locator("[data-work-unavailable]")).toContainText("No Instance is running");

	await work.getByRole("tab", { name: "Diff" }).click();
	await expect(work.getByRole("tabpanel", { name: "Diff" }).getByRole("alert")).toContainText(
		"has no Instance to read",
	);

	await work.getByRole("tab", { name: "Files" }).click();
	await expect(work.getByRole("tabpanel", { name: "Files" }).getByRole("alert")).toContainText(
		"has no Instance to read",
	);

	await work.getByRole("tab", { name: "Sessions" }).click();
	const card = work.locator("[data-session]");
	await expect(card).toContainText("builder · opencode");
	await expect(card.locator("[data-session-model]")).toContainText(
		"requested harness default · effective unknown",
	);
	await expect(card.locator("[data-session-continuity]")).toContainText("no Instance");

	await work.getByRole("tab", { name: "People" }).click();
	await expect(work.locator("[data-joined]")).toContainText("builder");
});

test("People shows joined Participants apart from named and anonymous followers", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	await viewing(page, workspace.name);
	const people = workPane(page);
	await people.getByRole("tab", { name: "People" }).click();

	await expect(people.locator("[data-joined]")).toContainText("builder");
	await expect(people.locator("[data-anonymous]")).toContainText("1 anonymous");

	const follow = `/operator/organizations/${ORGANIZATION}/workspaces/${workspace.name}/transcript`;
	await page.evaluate(async (transcript) => {
		const keep = async (query: string) => {
			const response = await fetch(`${transcript}?${query}`);
			const reader = response.body?.getReader();
			if (!reader) return;
			void (async () => {
				for (;;) {
					// oxlint-disable-next-line no-await-in-loop -- reading forever is what keeps the follower registered.
					const { done } = await reader.read();
					if (done) return;
				}
			})();
		};
		await Promise.all(["follow=true&as=reviewer", "follow=true"].map(keep));
	}, follow);

	await expect(people.locator('[data-follower="reviewer"]')).toBeVisible({ timeout: 20_000 });
	await expect(people.locator("[data-anonymous]")).toContainText("2 anonymous");
	await expect(people.locator("[data-follower]")).toHaveCount(1);
});

test("two browsers follow under their declared names, including one declared after loading", async ({
	browser,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const jack = await browser.newContext();
	const jill = await browser.newContext();
	await identified(jack, "jack");
	const first = await jack.newPage();
	const second = await jill.newPage();
	await viewing(first, workspace.name);
	await viewing(second, workspace.name);
	const people = workPane(first);
	await people.getByRole("tab", { name: "People" }).click();

	await expect(people.locator('[data-follower="jack"]')).toBeVisible({ timeout: 20_000 });
	await expect(people.locator("[data-anonymous]")).toContainText("1 anonymous");

	await second.getByLabel("Post").fill("not sent yet");
	await second.getByRole("button", { name: "Post" }).click();
	await second.getByLabel("Your name").fill("jill");
	await second.getByRole("button", { name: "Use this name" }).click();

	await expect(people.locator('[data-follower="jill"]')).toBeVisible({ timeout: 20_000 });
	await expect(people.locator('[data-follower="jack"]')).toBeVisible();
	await expect(people.locator("[data-anonymous]")).toHaveCount(0, { timeout: 20_000 });

	await jack.close();
	await jill.close();
});

class Reads {
	workspace: Record<string, unknown> | undefined;
	work: Record<string, unknown> | undefined;
	sessions: unknown[] | undefined;
	changes = new Map<string, unknown>();
	commits: unknown = { repositories: [] };
	stashes: unknown = { repositories: [] };
	files = new Map<string, unknown>();
	file = new Map<string, { status: number; contentType: string; body: string | Buffer }>();

	constructor(private readonly workspaceName: string) {}

	async install(page: Page): Promise<void> {
		const base = `/operator/organizations/${ORGANIZATION}/workspaces`;
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}`,
			async (route) => {
				if (this.workspace === undefined) {
					await route.continue();
					return;
				}
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.workspace),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/work`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(
						this.work ?? { state: "no_instance", branch: "main", pull_request: null },
					),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/sessions`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.sessions ?? []),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/changes`,
			async (route) => {
				const scope = new URL(route.request().url()).searchParams.get("scope") ?? "";
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.changes.get(scope) ?? { repositories: [] }),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/commits`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.commits),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/stashes`,
			async (route) => {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.stashes),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/files`,
			async (route) => {
				const path = new URL(route.request().url()).searchParams.get("path") ?? "";
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(
						this.files.get(path) ?? { path, entries: [], total: 0, truncated: false },
					),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/${this.workspaceName}/file`,
			async (route) => {
				const parameters = new URL(route.request().url()).searchParams;
				const key = `${parameters.get("path") ?? ""}${parameters.get("raw") === "true" ? "?raw" : ""}`;
				const answer = this.file.get(key);
				if (!answer) {
					await route.fulfill({
						status: 404,
						contentType: "application/json",
						body: JSON.stringify({ message: "no such file" }),
					});
					return;
				}
				await route.fulfill({
					status: answer.status,
					contentType: answer.contentType,
					body: answer.body,
				});
			},
		);
	}
}

function session(index: number, overrides: Record<string, unknown> = {}) {
	return {
		id: `00000000-0000-0000-0000-0000000001${index}`,
		name: `calm-river-abcdefg${index}`,
		workspace: "00000000-0000-0000-0000-0000000000ff",
		state: "working",
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
		changing_options: [],
		interrupting: null,
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

test("committed-but-unpushed, changed and untracked work show separately", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.workspace = {
		id: workspace.id,
		name: workspace.name,
		organization: ORGANIZATION,
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: {
			repositories: ["https://github.com/openkestrel/kestrel"],
			base: "main",
			branch: "kestrel/work",
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
		pull_requests: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				availability: "available",
				known: [
					{
						repository: "https://github.com/openkestrel/kestrel",
						number: 7,
						url: "https://github.com/openkestrel/kestrel/pull/7",
						title: "Keep work current",
						state: "open",
						head_branch: "kestrel/work",
						head_revision: "abcdef",
						updated_at: "2026-09-30T10:00:00Z",
						event: "00000000-0000-0000-0000-000000000090",
					},
				],
			},
		],
		unfinished_session: null,
	};
	reads.work = {
		state: "reported",
		reported_at: new Date().toISOString(),
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				git: "read",
				branch: "kestrel/work",
				changed: { files: 4, added: 12, removed: 3 },
				staged: { files: 1, added: 5, removed: 1 },
				committed: { commits: 2, added: 8, removed: 2 },
				pushed: null,
				untracked: 3,
				stashed: 1,
			},
		],
	};
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	await expect(work.getByText("pull request #7 open: Keep work current")).toBeVisible();
	await expect(work.locator('[data-work="pushed"]')).toHaveText("nothing pushed");
	await expect(work.locator('[data-work="committed"]')).toHaveText("2 commits +8 −2");
	await expect(work.locator('[data-work="staged"]')).toHaveText("1 file +5 −1");
	await expect(work.locator('[data-work="changed"]')).toHaveText("4 files +12 −3");
	await expect(work.locator('[data-work="untracked"]')).toHaveText("3 files");
	await expect(work.locator('[data-work="stashed"]')).toHaveText("1 stash");
	await expect(work.locator("[data-reported]")).toContainText("reported just now");
});

test("the Diff reads each scope and one file reads safely", async ({ page, request }) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.changes.set("unpublished", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: "diff --git a/x b/x\n+one\n",
				files: [{ path: "x", added: 1, removed: 0 }],
				truncated: true,
			},
		],
	});
	reads.changes.set("changed", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: "changed diff",
				files: [],
				truncated: false,
			},
		],
	});
	reads.changes.set("staged", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: "staged diff",
				files: [],
				truncated: false,
			},
		],
	});
	reads.changes.set("commit:abc1234", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: "commit diff",
				files: [],
				truncated: false,
			},
		],
	});
	reads.commits = {
		repositories: [
			{ repository: "https://github.com/openkestrel/kestrel", text: "abc1234 do a thing" },
		],
	};
	reads.stashes = {
		repositories: [{ repository: "https://github.com/openkestrel/kestrel", text: "stash@{0} wip" }],
	};
	reads.files.set("", {
		path: "",
		entries: [{ name: "kestrel", kind: "directory" }],
		total: 1,
		truncated: false,
	});
	reads.files.set("kestrel", {
		path: "kestrel",
		entries: [
			{ name: "src", kind: "directory" },
			{ name: "README.md", kind: "file", size: 1234, git: "tracked" },
			{ name: "logo.png", kind: "file", size: 4096, git: "untracked" },
		],
		total: 3,
		truncated: false,
	});
	reads.files.set("kestrel/src", {
		path: "kestrel/src",
		entries: [{ name: "main.rs", kind: "file", size: 512, git: "tracked" }],
		total: 5,
		truncated: true,
	});
	reads.file.set("kestrel/README.md", {
		status: 200,
		contentType: "application/json",
		body: JSON.stringify({ path: "kestrel/README.md", text: "# readme" }),
	});
	reads.file.set("kestrel/README.md?raw", {
		status: 200,
		contentType: "application/octet-stream",
		body: "# readme raw",
	});
	reads.file.set("kestrel/logo.png", {
		status: 200,
		contentType: "application/octet-stream",
		body: Buffer.from([0, 1, 2, 3]),
	});
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	await work.getByRole("tab", { name: "Diff" }).click();
	const diff = work.locator('[data-diff="https://github.com/openkestrel/kestrel"]');
	await expect(diff).toContainText("+one");
	await expect(diff.locator("[data-truncated]")).toBeVisible();

	await work.getByRole("button", { name: "Changed" }).click();
	await expect(diff).toContainText("changed diff");

	await work.getByRole("button", { name: "Staged" }).click();
	await expect(diff).toContainText("staged diff");

	await work.getByRole("button", { name: "Commit", exact: true }).click();
	await work.getByLabel("Commit revision").fill("abc1234");
	await work.getByRole("button", { name: "Read commit" }).click();
	await expect(diff).toContainText("commit diff");

	await work.getByRole("button", { name: "Commits" }).click();
	await expect(work.getByText("abc1234 do a thing")).toBeVisible();
	await work.getByRole("button", { name: "Stashes" }).click();
	await expect(work.getByText("stash@{0} wip")).toBeVisible();

	await work.getByRole("tab", { name: "Files" }).click();
	await work.getByRole("button", { name: "kestrel" }).click();
	await expect(work.getByRole("button", { name: /README\.md/ })).toBeVisible();
	await work.getByRole("button", { name: /README\.md/ }).click();
	await expect(work.locator("[data-file]")).toContainText("# readme");

	await work.getByRole("button", { name: "Raw" }).click();
	await expect(work.locator("[data-file]")).toContainText("# readme raw");

	await work.getByRole("button", { name: /logo\.png/ }).click();
	await expect(work.locator("[data-file]")).toContainText("binary content · 4 B");

	await work.getByRole("button", { name: "src" }).click();
	await expect(work.locator("[data-truncated]")).toContainText("showing 1 of 5 entries");
});

test("stale work and a lost conversation are stated plainly", async ({ page, request }) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.work = { state: "not_answering", message: "the Instance isn't answering" };
	reads.sessions = [
		session(1, {
			state: "ended",
			exit: { status: "failed", because: "the Session lost ACP continuity" },
			model: "claude-x",
			worked_model: "claude-y",
			title: "a conversation",
			usage: { context_used: 1200, context_size: 200_000, cost: null },
			options: [
				{
					id: "mode",
					name: "Mode",
					description: null,
					category: "mode",
					kind: "select",
					current: "plan",
					values: [],
					groups: [],
					warns_cache: false,
				},
			],
			commands: [{ name: "compact", description: "Compact", input_hint: null }],
		}),
	];
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	await expect(work.locator("[data-work-unavailable]")).toHaveText("the Instance isn't answering");

	await work.getByRole("tab", { name: "Sessions" }).click();
	const card = work.locator("[data-session]");
	await expect(card.locator("[data-session-outcome]")).toContainText(
		"failed: the Session lost ACP continuity",
	);
	await expect(card.locator("[data-session-model]")).toContainText(
		"requested claude-x · effective claude-y",
	);
	await expect(card.locator("[data-session-options]")).toContainText("Mode: plan");
	await expect(card.locator("[data-session-commands]")).toContainText("compact");
	await expect(card).toContainText("1.2k/200.0k context");
});
