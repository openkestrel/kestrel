import { AxeBuilder } from "@axe-core/playwright";
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

	await work.getByRole("tab", { name: "Changes" }).click();
	const changes = work.getByRole("tabpanel", { name: "Changes" });
	// The Commits read is refused too, so the Changes refusal is named by position.
	const refused = changes.getByRole("alert").first();
	await expect(refused).toContainText("has no Instance to read");
	const inspection = refused.locator('[data-step="inspect_resource"]');
	await expect(inspection).toBeVisible();
	await expect(inspection.getByRole("link", { name: /^Inspect the Workspace/ })).toBeVisible();
	await expect(inspection.getByRole("code")).toHaveText(/^kestrel workspace show /);
	await expect(inspection.getByRole("button", { name: /^Copy the command/ })).toBeVisible();

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
	changeReads: string[] = [];
	changesHeld: Promise<void> | undefined;
	changesFailure: { status: number; body: unknown } | undefined;
	changesUnavailable = false;
	commits: unknown = { repositories: [] };
	stashes: unknown = { repositories: [] };
	files = new Map<string, unknown>();
	file = new Map<string, { status: number; contentType: string; body: string | Buffer }>();

	constructor(private readonly workspaceName: string) {}

	holdChanges(): () => void {
		const held = Promise.withResolvers<void>();
		this.changesHeld = held.promise;
		return held.resolve;
	}

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
						this.work ?? {
							state: "no_instance",
							branch: "main",
							pull_request: null,
							last_report: { report: "none" },
							earlier_reports: [],
						},
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
				const parameters = new URL(route.request().url()).searchParams;
				const scope = parameters.get("scope") ?? "";
				const paths = parameters.getAll("path");
				this.changeReads.push(route.request().url());
				await this.changesHeld;
				if (this.changesUnavailable) {
					await route.continue();
					return;
				}
				if (this.changesFailure) {
					await route.fulfill({
						status: this.changesFailure.status,
						contentType: "application/json",
						body: JSON.stringify(this.changesFailure.body),
					});
					return;
				}
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(
						this.changes.get(`${scope}|${paths.join(",")}`) ??
							this.changes.get(scope) ?? { repositories: [] },
					),
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
		last_report: { report: "none" },
		earlier_reports: [],
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

test("the Changes read each scope, and a file expands in place to its diff", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	const repository = "https://github.com/openkestrel/kestrel";
	reads.changes.set("unpublished", {
		repositories: [
			{
				repository,
				diff: "diff --git a/one.rs b/one.rs\n+one\n",
				files: [{ path: "one.rs", added: 1, removed: 0 }],
				truncated: true,
			},
		],
	});
	// The operator filters patch text by the literal path it is asked for.
	reads.changes.set("unpublished|one.rs", {
		repositories: [
			{
				repository,
				diff: "diff --git a/one.rs b/one.rs\n+one only\n",
				files: [{ path: "one.rs", added: 1, removed: 0 }],
				truncated: false,
			},
		],
	});
	reads.changes.set("changed", {
		repositories: [
			{
				repository,
				diff: "diff --git a/two.rs b/two.rs\n+changed diff\n",
				files: [{ path: "two.rs", added: 2, removed: 0 }],
				truncated: false,
			},
		],
	});
	reads.changes.set("staged", {
		repositories: [
			{
				repository,
				diff: "diff --git a/three.rs b/three.rs\n+staged diff\n",
				files: [{ path: "three.rs", added: 3, removed: 0 }],
				truncated: false,
			},
		],
	});
	reads.changes.set("commit:abc1234", {
		repositories: [
			{
				repository,
				diff: "diff --git a/four.rs b/four.rs\n+commit diff\n",
				files: [{ path: "four.rs", added: 4, removed: 0 }],
				truncated: false,
			},
		],
	});
	reads.commits = {
		repositories: [{ repository, text: "abc1234 do a thing" }],
	};
	reads.stashes = {
		repositories: [{ repository, text: "stash@{0} wip" }],
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

	await work.getByRole("tab", { name: "Changes" }).click();
	const changes = work.getByRole("tabpanel", { name: "Changes" });
	const diff = changes.locator(`[data-diff="${repository}"]`);

	await expect(diff.locator("[data-diff-total]")).toHaveText("+1 −0");
	await expect(diff.locator("[data-truncated]")).toBeVisible();

	const one = changes.getByRole("button", { name: /one\.rs/ });
	await expect(one).toHaveAttribute("aria-expanded", "false");
	await one.click();
	await expect(one).toHaveAttribute("aria-expanded", "true");
	await expect(one).toBeFocused();
	await expect(changes.locator('[data-file-diff="one.rs"]')).toContainText("+one only");
	expect(reads.changeReads.some((read) => read.includes("path=one.rs"))).toBe(true);

	await one.click();
	await expect(one).toHaveAttribute("aria-expanded", "false");

	await changes.getByRole("button", { name: "Changed" }).click();
	await changes.getByRole("button", { name: /two\.rs/ }).click();
	await expect(changes.locator('[data-file-diff="two.rs"]')).toContainText("changed diff");

	await changes.getByRole("button", { name: "Staged" }).click();
	await changes.getByRole("button", { name: /three\.rs/ }).click();
	await expect(changes.locator('[data-file-diff="three.rs"]')).toContainText("staged diff");

	await changes.getByRole("button", { name: "Commit", exact: true }).click();
	await changes.getByLabel("Commit revision").fill("abc1234");
	await changes.getByRole("button", { name: "Read commit" }).click();
	await changes.getByRole("button", { name: /four\.rs/ }).click();
	await expect(changes.locator('[data-file-diff="four.rs"]')).toContainText("commit diff");

	await expect(changes.getByRole("region", { name: "Commits" })).toContainText(
		"abc1234 do a thing",
	);
	await changes.getByRole("button", { name: "Stashes" }).click();
	await expect(changes.getByText("stash@{0} wip")).toBeVisible();

	await work.getByRole("tab", { name: "Files" }).click();
	const files = work.getByRole("tabpanel", { name: "Files" });
	const tree = files.getByRole("tree");
	await expect(tree).toBeVisible();
	const kestrel = files.locator('[data-path="kestrel"]');
	await expect(kestrel).toHaveAttribute("aria-expanded", "false");
	await tree.getByRole("button", { name: "kestrel", exact: true }).click();
	await expect(kestrel).toHaveAttribute("aria-expanded", "true");

	const readme = files.locator('[data-path="kestrel/README.md"]');
	await expect(readme).toHaveAttribute("role", "treeitem");
	await readme.click();
	await expect(readme).toBeFocused();
	await expect(files.locator("[data-file]")).toContainText("# readme");

	await files.getByRole("button", { name: "Raw" }).click();
	await expect(files.locator("[data-file]")).toContainText("# readme raw");

	await files.locator('[data-path="kestrel/logo.png"]').click();
	await expect(files.locator("[data-file]")).toContainText("binary content · 4 B");

	await tree.getByRole("button", { name: "src", exact: true }).click();
	await expect(files.locator("[data-truncated]")).toContainText("showing 1 of 5 entries");
});

test("what a nested directory says about itself stands outside the tree's groups", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.files.set("", {
		path: "",
		entries: [
			{ name: "bare", kind: "directory" },
			{ name: "full", kind: "directory" },
		],
		total: 2,
		truncated: false,
	});
	reads.files.set("full", {
		path: "full",
		entries: [{ name: "main.rs", kind: "file", size: 512, git: "tracked" }],
		total: 5,
		truncated: true,
	});
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);
	await work.getByRole("tab", { name: "Files" }).click();
	const tree = work.getByRole("tabpanel", { name: "Files" }).getByRole("tree");

	await tree.getByRole("button", { name: "bare", exact: true }).click();
	const bare = tree.getByRole("treeitem", { expanded: true });
	await expect(bare.getByText("This directory is empty.")).toBeVisible();
	await expect(bare.getByRole("group")).toHaveCount(0);

	await tree.getByRole("button", { name: "Collapse bare" }).click();
	await tree.getByRole("button", { name: "full", exact: true }).click();
	const full = tree.getByRole("treeitem", { expanded: true });
	await expect(full.getByRole("group").getByRole("treeitem")).toHaveCount(1);
	await expect(full.getByText("showing 1 of 5 entries")).toBeVisible();
	await expect(full.getByRole("group").getByText("showing 1 of 5 entries")).toHaveCount(0);

	const { violations } = await new AxeBuilder({ page }).include('[role="tree"]').analyze();
	expect(violations.map(({ id }) => id)).toEqual([]);
});

test("a long diff and a long file stay keyboard-reachable", async ({ page, request }) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	const long = Array.from({ length: 200 }, (_, index) => `+line ${index}`).join("\n");
	reads.changes.set("unpublished", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: long,
				files: [{ path: "long.rs", added: 200, removed: 0 }],
				truncated: false,
			},
		],
	});
	reads.files.set("", {
		path: "",
		entries: [{ name: "kestrel", kind: "directory" }],
		total: 1,
		truncated: false,
	});
	reads.files.set("kestrel", {
		path: "kestrel",
		entries: [{ name: "README.md", kind: "file", size: long.length, git: "tracked" }],
		total: 1,
		truncated: false,
	});
	reads.file.set("kestrel/README.md", {
		status: 200,
		contentType: "application/json",
		body: JSON.stringify({ path: "kestrel/README.md", text: long }),
	});
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	await work.getByRole("tab", { name: "Changes" }).click();
	const changes = work.getByRole("tabpanel", { name: "Changes" });
	await changes.getByRole("button", { name: /long\.rs/ }).click();
	await expect(changes.locator('[data-file-diff="long.rs"]')).toContainText("+line 199");
	await reachable(page);

	await work.getByRole("tab", { name: "Files" }).click();
	const files = work.getByRole("tabpanel", { name: "Files" });
	await files.getByRole("button", { name: "kestrel", exact: true }).click();
	await files.locator('[data-path="kestrel/README.md"]').click();
	await expect(files.locator("[data-file]")).toContainText("+line 199");
	await reachable(page);
});

test("the Changes read says when it is loading, idle, empty and failed", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	const release = reads.holdChanges();
	reads.changes.set("unpublished", {
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				diff: "",
				files: [],
				truncated: false,
			},
		],
	});
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);
	await work.getByRole("tab", { name: "Changes" }).click();
	const changes = work.getByRole("tabpanel", { name: "Changes" });
	const reading = changes.locator("[data-changes]");

	await expect(reading).toHaveAttribute("aria-busy", "true");
	release();
	await expect(reading).toContainText("no changes");
	await expect(reading).toHaveAttribute("aria-busy", "false");

	await changes.getByRole("button", { name: "Commit", exact: true }).click();
	await expect(changes.getByText("Name a revision to read its diff.")).toBeVisible();
	await expect(reading).toHaveAttribute("aria-busy", "false");

	// A scope with nothing to read says so, rather than showing a stale list.
	await changes.getByRole("button", { name: "Changed" }).click();
	await expect(changes.locator("[data-no-changes]")).toHaveText("No repository to read.");

	// A failed read names the error and offers the typed recovery.
	reads.changesFailure = { status: 422, body: { message: "the diff read blew up" } };
	await changes.getByRole("button", { name: "Staged" }).click();
	const alert = changes.getByRole("alert");
	await expect(alert).toContainText("the diff read blew up");
	await expect(alert.locator('[data-step="retry_read"]')).toBeVisible();
	await expect(alert.getByRole("button", { name: "Read again" })).toBeVisible();
});

test("a live Changes read never falls back to the last repository summary", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.work = {
		state: "reported",
		reported_at: new Date().toISOString(),
		last_report: { report: "none" },
		earlier_reports: [],
		repositories: [
			{
				repository: "https://github.com/openkestrel/kestrel",
				git: "read",
				branch: "kestrel/work",
				changed: { files: 4, added: 12, removed: 3 },
				staged: { files: 0, added: 0, removed: 0 },
				committed: { commits: 0, added: 0, removed: 0 },
				pushed: null,
				untracked: 0,
				stashed: 0,
			},
		],
	};
	// The live read is refused: this Workspace has no Instance to read.
	reads.changesUnavailable = true;
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	// The Work tab keeps showing the report it was last given, as history.
	await expect(work.locator('[data-work="changed"]')).toHaveText("4 files +12 −3");

	await work.getByRole("tab", { name: "Changes" }).click();
	const changes = work.getByRole("tabpanel", { name: "Changes" });
	await expect(changes.getByRole("alert").first()).toContainText("has no Instance to read");
	await expect(changes.locator('[data-step="inspect_resource"]')).toBeVisible();
	await expect(changes).not.toContainText("4 files +12 −3");
});

// Chromium lets a keyboard into any overflowing scroller, so only axe's rule tells the region
// is reachable in every browser.
async function reachable(page: Page): Promise<void> {
	const { violations } = await new AxeBuilder({ page })
		.withRules(["scrollable-region-focusable"])
		.analyze();
	expect(violations.flatMap(({ nodes }) => nodes.map((node) => node.html.slice(0, 120)))).toEqual(
		[],
	);
}

test("stale work and a lost conversation are stated plainly", async ({ page, request }) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.work = {
		state: "not_answering",
		message: "the Instance isn't answering",
		last_report: { report: "none" },
		earlier_reports: [],
	};
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

test("a failed Session's read shows why it failed and how to repair it, as no refusal", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, "an opening brief");
	const reads = new Reads(workspace.name);
	reads.sessions = [
		session(1, {
			state: "ended",
			exit: { status: "failed", because: "authentication required" },
			diagnostic: {
				kind: "authentication_failed",
				message: "the claude harness needed a sign-in before it would work",
				field: null,
				context: {
					session: "s-1",
					harness: "claude",
					image: null,
					evidence: {
						kind: "authentication_required",
						code: -32000,
						methods: ["claude-login"],
						method: "claude-login",
					},
					sign_in: null,
					expired: null,
					covered: null,
				},
				next_steps: [
					{ action: "sign_in", harness: "claude", method: "claude-login", sign_in: null },
					{
						action: "inspect_resource",
						resource: "session",
						reference: "s-1",
						organization: ORGANIZATION,
					},
				],
			},
		}),
	];
	await reads.install(page);
	await viewing(page, workspace.name);
	const work = workPane(page);

	await work.getByRole("tab", { name: "Sessions" }).click();
	const why = work.getByRole("region", { name: "Why it failed" });
	await expect(why).toContainText("the claude harness needed a sign-in before it would work");
	await expect(why).toContainText("claude-login");
	await expect(why).not.toContainText("Expired");
	await expect(why.locator('[data-step="sign_in"]')).toHaveText(
		"Sign in to the claude harness with claude-login",
	);
	const inspection = why.locator('[data-step="inspect_resource"]');
	await expect(inspection.getByText(`Inspect the Session s-1 in ${ORGANIZATION}`)).toBeVisible();
	await expect(inspection.getByRole("code")).toHaveText(
		/^kestrel session show s-1 --organization acme --control-plane /,
	);
	await expect(inspection.getByRole("button", { name: /^Copy the command/ })).toBeVisible();
	await expect(work.getByRole("alert")).toHaveCount(0);
});
