import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import {
	expect,
	test,
	type APIRequestContext,
	type BrowserContext,
	type Page,
	type Route,
} from "@playwright/test";

test.setTimeout(600_000);
test.describe.configure({ mode: "serial" });

const ORGANIZATION = "acme";
const ROOT = resolve(import.meta.dirname, "../../..");
const CLI = resolve(ROOT, "target/debug/kestrel");
const CONTROL_PLANE = "http://127.0.0.1:17718";

const followers: ChildProcess[] = [];

test.beforeAll(async ({ request }) => {
	execFileSync("cargo", ["build", "--quiet", "--locked", "--package", "kestrel-client"], {
		cwd: ROOT,
		stdio: "inherit",
	});
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

test.afterAll(() => {
	for (const follower of followers) follower.kill();
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

async function posting(
	request: APIRequestContext,
	workspace: string,
	participant: string,
	message: string,
): Promise<void> {
	const response = await request.post(
		`/operator/organizations/${ORGANIZATION}/workspaces/${workspace}/messages`,
		{ data: { participant, message } },
	);
	expect(response.ok(), await response.text()).toBe(true);
}

async function visiting(page: Page, workspace: string): Promise<void> {
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace}`);
	await expect(page.locator("[data-session-header]")).toBeVisible();
	await expect(page.locator("[data-composer]")).toBeVisible();
}

// The client binary as a second, named follower: it holds a follow lease the browser can see in
// the presence event, and prints each entry it is handed as JSON.
function following(workspace: string, participant: string): { lines: string[]; errors: string[] } {
	const child = spawn(
		CLI,
		[
			"--organization",
			ORGANIZATION,
			"workspace",
			"transcript",
			workspace,
			"--follow",
			"--as-participant",
			participant,
			"--json",
			"seq,appended_at,entry",
		],
		{
			env: { ...process.env, KESTREL_CONTROL_PLANE: CONTROL_PLANE },
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	followers.push(child);
	const lines: string[] = [];
	child.stdout?.setEncoding("utf8");
	child.stdout?.on("data", (chunk: string) => {
		lines.push(...chunk.split("\n").filter((line) => line.trim() !== ""));
	});
	const errors: string[] = [];
	child.stderr?.setEncoding("utf8");
	child.stderr?.on("data", (chunk: string) => {
		errors.push(...chunk.split("\n").filter((line) => line.trim() !== ""));
	});
	return { lines, errors };
}

async function identified(context: BrowserContext, name: string): Promise<void> {
	const remembered: [string, string] = ["kestrel:participant", name];
	await context.addInitScript(([key, value]: [string, string]) => {
		localStorage.setItem(key, value);
	}, remembered);
}

const WORKSPACE_ID = "11111111-1111-1111-1111-111111111111";
const SESSION_ID = "22222222-2222-2222-2222-222222222222";

function workspaceRead(name: string, overrides: Record<string, unknown> = {}) {
	return {
		id: WORKSPACE_ID,
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

function sessionRead(overrides: Record<string, unknown> = {}) {
	return {
		id: SESSION_ID,
		name: "calm-river-abcdefgh",
		workspace: WORKSPACE_ID,
		state: "waiting",
		preparing: null,
		exit: null,
		outcome_message: null,
		instance: null,
		supervisor: null,
		agent: "builder",
		harness: "opencode",
		model: "scripted-mini",
		mode: null,
		thought_level: null,
		worked_model: "scripted-mini",
		title: null,
		options: [],
		changing_options: [],
		commands: [
			{ name: "compact", description: "Compact the conversation", input_hint: "/compact" },
		],
		interrupting: null,
		enqueued_at: "2026-09-30T10:00:00Z",
		started_at: null,
		ended_at: null,
		lease_expires_at: null,
		connected_at: null,
		supervisor_version: null,
		usage: { context_used: 1_200, context_size: 200_000, cost: null },
		tools: [],
		message_buffering: false,
		thought_buffering: false,
		...overrides,
	};
}

function option(overrides: Record<string, unknown> = {}) {
	return {
		id: "model",
		name: "Model",
		description: null,
		category: "model",
		kind: "select",
		current: "scripted-mini",
		values: [
			{ value: "scripted-mini", name: "scripted-mini", description: null },
			{ value: "scripted-max", name: "scripted-max", description: null },
		],
		groups: [],
		warns_cache: true,
		...overrides,
	};
}

type Answer = { status: number; body: unknown };

// The control plane cannot force a held message, a working Turn or a harness refusal without a
// supervisor, so this spec scripts the Session and Workspace reads and the writes that answer
// them; the Transcript, presence and the name rule stay the real control plane's.
class Scripted {
	readonly name: string;
	workspace: Record<string, unknown>;
	session: Record<string, unknown>;
	requests: { method: string; path: string; body: unknown }[] = [];
	post: Answer = { status: 200, body: { session: null, held_message: null } };
	edit: Answer = { status: 200, body: null };
	withdraw: Answer = { status: 204, body: null };
	interrupt: Answer = { status: 202, body: null };
	option: Answer = { status: 202, body: null };

	constructor(name: string) {
		this.name = name;
		this.workspace = workspaceRead(name);
		this.session = sessionRead();
	}

	async install(page: Page): Promise<void> {
		const workspace = `/operator/organizations/${ORGANIZATION}/workspaces/${this.name}`;
		const session = `/operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}`;
		await page.route(
			(url) => url.pathname === workspace,
			(route) => this.json(route, this.workspace),
		);
		await page.route(
			(url) => url.pathname === `${workspace}/sessions`,
			(route) => this.json(route, [this.session]),
		);
		await page.route(
			(url) => url.pathname === session,
			(route) => this.json(route, this.session),
		);
		await page.route(
			(url) => url.pathname.startsWith(`${workspace}/messages`),
			(route) => this.answer(route, this.messageAnswer(route)),
		);
		await page.route(
			(url) => url.pathname === `${session}/interrupt`,
			(route) => this.answer(route, this.interrupt),
		);
		await page.route(
			(url) => url.pathname === `${session}/options`,
			(route) => this.answer(route, this.option),
		);
	}

	messageAnswer(route: Route): Answer {
		const method = route.request().method();
		if (method === "PUT") return this.edit;
		if (method === "DELETE") return this.withdraw;
		return this.post;
	}

	private async json(route: Route, body: unknown): Promise<void> {
		await route.fulfill({
			status: 200,
			contentType: "application/json",
			body: JSON.stringify(body),
		});
	}

	private async answer(route: Route, answer: Answer): Promise<void> {
		const request = route.request();
		this.requests.push({
			method: request.method(),
			path: new URL(request.url()).pathname,
			body: request.postDataJSON(),
		});
		if (answer.status === 204) {
			await route.fulfill({ status: 204, body: "" });
			return;
		}
		await route.fulfill({
			status: answer.status,
			contentType: "application/json",
			body: JSON.stringify(answer.body),
		});
	}
}

test("the first write asks for a name and remembers it in this browser", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	await visiting(page, workspace);

	await page.getByLabel("Post").fill("hello from the browser");
	await page.getByRole("button", { name: "Post" }).click();

	await expect(page.getByLabel("Your name")).toBeVisible();
	await page.getByLabel("Your name").fill("jack");
	await page.getByRole("button", { name: "Use this name" }).click();
	await expect(page.getByText("writing as jack")).toBeVisible();
	expect(await page.evaluate(() => localStorage.getItem("kestrel:participant"))).toBe("jack");

	await page.getByRole("button", { name: "Post" }).click();
	await expect(
		page.locator("[data-seq]").filter({ hasText: "jack: hello from the browser" }),
	).toBeVisible({ timeout: 20_000 });

	await page.reload();
	await expect(page.getByText("writing as jack")).toBeVisible();

	await page.getByRole("button", { name: "change" }).click();
	await page.getByLabel("Your name").fill("jill");
	await page.getByRole("button", { name: "Use this name" }).click();
	expect(await page.evaluate(() => localStorage.getItem("kestrel:participant"))).toBe("jill");
});

test("two named people post in order, and a CLI follower is handed both", async ({
	browser,
	request,
}) => {
	const workspace = await opened(request);
	const jack = await browser.newContext();
	const jill = await browser.newContext();
	await identified(jack, "jack");
	await identified(jill, "jill");
	const first = await jack.newPage();
	const second = await jill.newPage();
	await visiting(first, workspace);
	await visiting(second, workspace);

	const follower = following(workspace, "observer");
	await posting(request, workspace, "jack", "first from jack");
	await posting(request, workspace, "jill", "second from jill");

	await Promise.all(
		[first, second].flatMap((page) => [
			expect(page.locator("[data-seq]").filter({ hasText: "jack: first from jack" })).toBeVisible({
				timeout: 20_000,
			}),
			expect(page.locator("[data-seq]").filter({ hasText: "jill: second from jill" })).toBeVisible({
				timeout: 20_000,
			}),
		]),
	);

	await expect
		.poll(
			() =>
				follower.lines.some((line) => line.includes("first from jack")) &&
				follower.lines.some((line) => line.includes("second from jill"))
					? "both"
					: `lines=${follower.lines.join(" | ")} errors=${follower.errors.join(" | ")}`,
			{ timeout: 20_000 },
		)
		.toBe("both");
	const order = follower.lines.findIndex((line) => line.includes("first from jack"));
	const later = follower.lines.findIndex((line) => line.includes("second from jill"));
	expect(later).toBeGreaterThan(order);

	await jack.close();
	await jill.close();
});

test("the header names the followers watching the Workspace", async ({ page, request }) => {
	const workspace = await opened(request);
	following(workspace, "jill");
	await visiting(page, workspace);

	await expect(page.locator("[data-session-followers]")).toContainText("jill", { timeout: 20_000 });
});

test("a working Session labels the post and lets its author amend a Held Message", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 4,
				participant: "jack",
				message: "hold this",
				posted_at: new Date(Date.now() - 120_000).toISOString(),
				edited_at: null,
			},
			{
				id: 5,
				participant: "jill",
				message: "and this",
				posted_at: new Date().toISOString(),
				edited_at: new Date().toISOString(),
			},
		],
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "working", preparing: null },
	});
	scripted.session = sessionRead({ state: "working" });
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await expect(page.getByRole("button", { name: "Add to next turn" })).toBeVisible();
	const held = page.locator("[data-held-messages]");
	await expect(held.locator('[data-held-message="4"]')).toContainText("jack");
	await expect(held.locator('[data-held-message="4"]')).toContainText("2 minutes ago");
	await expect(held.locator('[data-held-message="5"]')).toContainText("edited");
	await expect(held.locator('[data-held-message="5"]').getByRole("button")).toHaveCount(0);

	const { violations } = await new AxeBuilder({ page })
		.include("[data-session-header]")
		.include("[data-composer]")
		.analyze();
	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);

	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 4,
				participant: "jack",
				message: "hold this, edited",
				posted_at: new Date(Date.now() - 120_000).toISOString(),
				edited_at: new Date().toISOString(),
			},
		],
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "working", preparing: null },
	});
	await held.locator('[data-held-message="4"]').getByRole("button", { name: "Edit" }).click();
	await page.getByLabel("Edit the held message").fill("hold this, edited");
	await page.getByRole("button", { name: "Save" }).click();

	await expect(held.locator('[data-held-message="4"] [data-held-text]')).toHaveText(
		"hold this, edited",
	);
	expect(scripted.requests).toContainEqual({
		method: "PUT",
		path: `/operator/organizations/${ORGANIZATION}/workspaces/${workspace}/messages/4`,
		body: { participant: "jack", message: "hold this, edited" },
	});

	scripted.workspace = workspaceRead(workspace, {
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "working", preparing: null },
	});
	await held.locator('[data-held-message="4"]').getByRole("button", { name: "Withdraw" }).click();

	await expect(held.locator('[data-held-message="4"]')).toHaveCount(0);
	expect(scripted.requests).toContainEqual({
		method: "DELETE",
		path: `/operator/organizations/${ORGANIZATION}/workspaces/${workspace}/messages/4`,
		body: { participant: "jack" },
	});
});

test("a stale amendment refusal is shown with its field and phase", async ({ page, request }) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 7,
				participant: "jack",
				message: "too late",
				posted_at: new Date().toISOString(),
				edited_at: null,
			},
		],
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "working", preparing: null },
	});
	scripted.session = sessionRead({ state: "working" });
	scripted.edit = {
		status: 409,
		body: {
			message: "the held message was taken by the Turn",
			field: "id",
			phase: "working",
		},
	};
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await page.locator('[data-held-message="7"]').getByRole("button", { name: "Edit" }).click();
	await page.getByLabel("Edit the held message").fill("still here");
	await page.getByRole("button", { name: "Save" }).click();

	const refusal = page.getByRole("alert").filter({ hasText: "taken by the Turn" });
	await expect(refusal).toBeVisible();
	await expect(refusal).toContainText("id");
	await expect(refusal).toContainText("working");
});

test("Send now reports the post that landed and the interrupt that did not", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "working", preparing: null },
	});
	scripted.session = sessionRead({ state: "working" });
	scripted.interrupt = {
		status: 409,
		body: { message: "the session is waiting", phase: "waiting" },
	};
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await expect(page.getByRole("button", { name: "Interrupt" })).toBeEnabled();
	await page.getByLabel("Add to next turn").fill("stop after this");
	await page.getByRole("button", { name: "Send now" }).click();

	await expect(
		page.getByText("The message was posted, but the Turn was not interrupted"),
	).toBeVisible();
	const refusal = page.getByRole("alert").filter({ hasText: "the session is waiting" });
	await expect(refusal).toContainText("waiting");
	expect(scripted.requests.map(({ method, path }) => `${method} ${path}`)).toEqual([
		`POST /operator/organizations/${ORGANIZATION}/workspaces/${workspace}/messages`,
		`POST /operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}/interrupt`,
	]);

	scripted.session = sessionRead({ state: "waiting" });
	await page.reload();
	await expect(page.getByRole("button", { name: "Interrupt" })).toBeDisabled();
});

test("an option change warns about the cache before it is confirmed", async ({ page, request }) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "waiting", preparing: null },
	});
	scripted.session = sessionRead({
		options: [
			option(),
			option({
				id: "mode",
				name: "Mode",
				category: "mode",
				current: "build",
				values: [
					{ value: "build", name: "Build", description: null },
					{ value: "plan", name: "Plan", description: null },
				],
				warns_cache: false,
			}),
		],
	});
	scripted.option = {
		status: 202,
		body: sessionRead({
			changing_options: [
				{ option: "model", category: "model", value: "scripted-max", participant: "jack" },
			],
		}),
	};
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await expect(page.locator('[data-option="model"] [data-option-current]')).toHaveText(
		"scripted-mini",
	);
	await expect(page.locator("[data-session-usage]")).toContainText("1,200 of 200,000 tokens");
	await page.locator('[data-option="model"] [data-option-value="scripted-max"]').click();

	const confirm = page.locator("[data-option-confirm]");
	await expect(confirm).toContainText("1,200 tokens");
	scripted.session = sessionRead({
		options: [
			option(),
			option({
				id: "mode",
				name: "Mode",
				category: "mode",
				current: "build",
				values: [
					{ value: "build", name: "Build", description: null },
					{ value: "plan", name: "Plan", description: null },
				],
				warns_cache: false,
			}),
		],
		changing_options: [
			{ option: "model", category: "model", value: "scripted-max", participant: "jack" },
		],
	});
	await confirm.getByRole("button", { name: "Change" }).click();

	await expect(page.locator("[data-changing-option]")).toContainText(
		"jack is changing model to scripted-max",
	);
	expect(scripted.requests).toContainEqual({
		method: "POST",
		path: `/operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}/options`,
		body: { participant: "jack", category: "model", value: "scripted-max" },
	});

	scripted.session = sessionRead({
		state: "working",
		options: [option()],
	});
	await page.reload();
	await expect(
		page.locator('[data-option="model"] [data-option-value="scripted-max"]'),
	).toBeDisabled();
	await expect(page.locator("[data-option-note]")).toHaveCount(0);

	scripted.session = sessionRead({
		options: [option({ warns_cache: false })],
	});
	scripted.option = {
		status: 409,
		body: { message: "the session is working", phase: "working" },
	};
	await page.reload();
	await page.locator('[data-option="model"] [data-option-value="scripted-max"]').click();

	const refusal = page.getByRole("alert").filter({ hasText: "the session is working" });
	await expect(refusal).toContainText("working");
});

test("Shift+Tab cycles the mode and Escape leaves the composer, both announced", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "waiting", preparing: null },
	});
	scripted.session = sessionRead({
		options: [
			option({
				id: "mode",
				name: "Mode",
				category: "mode",
				current: "build",
				values: [
					{ value: "build", name: "Build", description: null },
					{ value: "plan", name: "Plan", description: null },
				],
				warns_cache: false,
			}),
		],
	});
	scripted.option = { status: 202, body: sessionRead({}) };
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	const input = page.getByLabel("Post");
	await input.focus();
	await input.press("Shift+Tab");

	await expect(page.locator("[data-announcement]")).toHaveText("mode plan");
	expect(scripted.requests).toContainEqual({
		method: "POST",
		path: `/operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}/options`,
		body: { participant: "jack", category: "mode", value: "plan" },
	});

	await input.press("Escape");
	await expect(page.locator("[data-announcement]")).toHaveText("Left the composer");
	await expect(input).not.toBeFocused();
});
