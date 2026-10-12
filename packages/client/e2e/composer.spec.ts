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
test.use({ actionTimeout: 10_000 });

const ORGANIZATION = "acme";
const ROOT = resolve(import.meta.dirname, "../../..");
const CLI = resolve(ROOT, "target/debug/kestrel");
const CONTROL_PLANE = "http://127.0.0.1:17718";

const followers: ChildProcess[] = [];

test.beforeAll(async ({ request }) => {
	// test.setTimeout above covers only the tests; a cold cargo build needs its own raised hook timeout.
	test.setTimeout(120_000);
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

async function opened(request: APIRequestContext, briefed = true): Promise<string> {
	const response = await request.post(`/operator/organizations/${ORGANIZATION}/workspaces`, {
		data: {
			project: "kestrel",
			agent: "builder",
			...(briefed ? { brief: "an opening brief" } : {}),
		},
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
		],
		{
			env: { ...process.env, KESTREL_CONTROL_PLANE: CONTROL_PLANE },
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	followers.push(child);
	const lines: string[] = [];
	child.stdout.setEncoding("utf8");
	child.stdout.on("data", (chunk: string) => {
		lines.push(...chunk.split("\n").filter((line) => line.trim() !== ""));
	});
	const errors: string[] = [];
	child.stderr.setEncoding("utf8");
	child.stderr.on("data", (chunk: string) => {
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
		sign_in_method: null,
		supplied: [],
		diagnostic: null,
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
		units: [],
		last_activity_at: null,
		observation: { availability: "unavailable", last: null },
		depends_on: [],
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

function modeOption(warns_cache: boolean) {
	return option({
		id: "mode",
		name: "Mode",
		category: "mode",
		current: "build",
		values: [
			{ value: "build", name: "Build", description: null },
			{ value: "plan", name: "Plan", description: null },
		],
		warns_cache,
	});
}

function modelChoice(page: Page) {
	return page.getByRole("button", { name: /^Model:/ });
}

async function choosingModel(page: Page, name: string): Promise<void> {
	await modelChoice(page).click();
	await page.getByRole("dialog").getByRole("option", { name }).click();
}

type Answer = { status: number; body: unknown };

function conflict(message: string, state: string, field: string | null = null) {
	return {
		kind: "state_conflict",
		message,
		field,
		context: {
			operation: "set_session_option",
			resource: "session",
			reference: SESSION_ID,
			organization: ORGANIZATION,
			state,
			holding_session: null,
		},
		next_steps: [
			{
				action: "inspect_resource",
				resource: "session",
				reference: SESSION_ID,
				organization: ORGANIZATION,
			},
		],
	};
}

// Held messages, working Turns and harness refusals need a supervisor, so the Session and
// Workspace reads and writes are scripted while the Transcript, presence and name rule stay real.
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
	optionGate: Promise<void> | null = null;
	interruptGate: Promise<void> | null = null;

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
			(route) => this.answer(route, this.interrupt, this.interruptGate),
		);
		await page.route(
			(url) => url.pathname === `${session}/options`,
			(route) => this.answer(route, this.option, this.optionGate),
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

	private async answer(route: Route, answer: Answer, gate?: Promise<void> | null): Promise<void> {
		const request = route.request();
		this.requests.push({
			method: request.method(),
			path: new URL(request.url()).pathname,
			body: request.postDataJSON(),
		});
		await gate;
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
	await expect(page.getByLabel("Your name")).toBeFocused();
	await page.getByLabel("Your name").fill("jack");
	await page.getByRole("button", { name: "Use this name" }).click();
	await expect(page.getByText("writing as jack")).toBeVisible();
	await expect(page.getByRole("textbox", { name: "Post", exact: true })).toBeFocused();
	expect(await page.evaluate(() => localStorage.getItem("kestrel:participant"))).toBe("jack");

	await page.getByRole("button", { name: "Post" }).click();
	await expect(
		page.getByRole("article", { name: "jack" }).filter({ hasText: "hello from the browser" }),
	).toBeVisible({ timeout: 20_000 });

	await page.reload();
	await expect(page.getByText("writing as jack")).toBeVisible();
	await expect(
		page.getByRole("article", { name: "jack" }).filter({ hasText: "hello from the browser" }),
	).toBeVisible({ timeout: 20_000 });

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
			expect(
				page.getByRole("article", { name: "jack" }).filter({ hasText: "first from jack" }),
			).toBeVisible({
				timeout: 20_000,
			}),
			expect(
				page.getByRole("article", { name: "jill" }).filter({ hasText: "second from jill" }),
			).toBeVisible({
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

test("a stale amendment refusal is shown with its field and state", async ({ page, request }) => {
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
		body: conflict("the held message was taken by the Turn", "working", "id"),
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
		body: conflict("the session is waiting", "waiting"),
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
	await expect(page.getByRole("button", { name: "Send now" })).toHaveCount(0);
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

	await expect(modelChoice(page)).toHaveText("scripted-mini");
	await expect(
		page.getByRole("button", { name: /Context: 0.6%, 1,200 of 200,000 tokens/ }),
	).toBeVisible();
	await choosingModel(page, "scripted-max");

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
	await expect(modelChoice(page)).toBeDisabled();
	await expect(page.locator("[data-option-note]")).toHaveCount(0);

	scripted.session = sessionRead({
		options: [option({ warns_cache: false })],
	});
	scripted.option = {
		status: 409,
		body: conflict("the session is working", "working"),
	};
	await page.reload();
	await choosingModel(page, "scripted-max");

	const refusal = page.getByRole("alert").filter({ hasText: "the session is working" });
	await expect(refusal).toContainText("working");
});

const OFFERED = [
	{
		group: "anthropic",
		name: "Anthropic",
		values: [
			{ value: "anthropic/claude-sonnet-4-5", name: "Claude Sonnet 4.5", description: null },
			{ value: "anthropic/claude-opus-4-5", name: "Claude Opus 4.5", description: "slower" },
		],
	},
	{
		group: "house",
		name: "House",
		values: [{ value: "house/small", name: "House Small", description: null }],
	},
];

test("the Model Selector searches what the harness offers, takes the keyboard and follows the harness", async ({
	page,
	request,
	baseURL,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	const offering = (current: string, groups = OFFERED) =>
		sessionRead({
			options: [option({ current, values: [], groups, warns_cache: false })],
		});
	scripted.session = offering("anthropic/claude-sonnet-4-5");
	scripted.option = { status: 202, body: scripted.session };
	const elsewhere: string[] = [];
	page.on("request", (sent) => {
		if (!sent.url().startsWith(baseURL ?? "")) elsewhere.push(sent.url());
	});
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await expect(modelChoice(page)).toHaveText("Claude Sonnet 4.5");
	await expect(modelChoice(page).locator("img")).toHaveJSProperty("complete", true);
	expect(
		await modelChoice(page)
			.locator("img")
			.evaluate((mark: HTMLImageElement) => mark.naturalWidth),
	).toBeGreaterThan(0);

	await modelChoice(page).focus();
	await page.keyboard.press("Enter");
	const dialog = page.getByRole("dialog", { name: "Choose a model the harness offers" });
	await expect(dialog.getByRole("combobox")).toBeFocused();
	await expect(dialog.getByRole("option")).toHaveText([
		"Claude Sonnet 4.5",
		"Claude Opus 4.5slower",
		"House Small",
	]);
	await expect(dialog.getByRole("group", { name: "Anthropic" }).getByRole("option")).toHaveCount(2);
	await expect(dialog.getByRole("option", { name: "House Small" }).locator("img")).toHaveCount(0);

	const { violations } = await new AxeBuilder({ page }).analyze();
	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);

	// Nothing the person did: the harness offers another model while the selector is open.
	scripted.session = offering("anthropic/claude-sonnet-4-5", [
		...OFFERED,
		{
			group: "late",
			name: "Late",
			values: [{ value: "late/arrival", name: "Late Arrival", description: null }],
		},
	]);
	await page.evaluate(() => window.dispatchEvent(new Event("visibilitychange")));
	await expect(dialog.getByRole("group", { name: "Late" }).getByRole("option")).toHaveText([
		"Late Arrival",
	]);

	await page.keyboard.type("nothing like it");
	await expect(dialog).toContainText("The harness offers no model matching that.");
	await dialog.getByRole("combobox").fill("opus");
	await expect(dialog.getByRole("option")).toHaveText(["Claude Opus 4.5slower"]);

	scripted.session = offering("anthropic/claude-opus-4-5", [
		...OFFERED,
		{
			group: "openai",
			name: "OpenAI",
			values: [{ value: "openai/gpt-5.4", name: "GPT-5.4", description: null }],
		},
	]);
	await page.keyboard.press("Enter");
	await expect(dialog).toHaveCount(0);
	expect(scripted.requests).toContainEqual({
		method: "POST",
		path: `/operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}/options`,
		body: { participant: "jack", category: "model", value: "anthropic/claude-opus-4-5" },
	});

	await expect(modelChoice(page)).toHaveText("Claude Opus 4.5");
	await modelChoice(page).click();
	await expect(dialog.getByRole("group", { name: "OpenAI" }).getByRole("option")).toHaveText([
		"GPT-5.4",
	]);
	await expect(dialog.getByRole("option", { name: "Claude Opus 4.5" })).toHaveAttribute(
		"data-checked",
		"true",
	);
	await page.keyboard.press("Escape");
	await expect(dialog).toHaveCount(0);
	await expect(modelChoice(page)).toBeFocused();
	expect(scripted.requests.filter((sent) => sent.path.endsWith("/options"))).toHaveLength(1);
	expect(elsewhere).toEqual([]);
});

test("a refused model change stays explained beside the model the harness still runs", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.session = sessionRead({ options: [option({ warns_cache: false })] });
	scripted.option = {
		status: 409,
		body: conflict("the harness refused scripted-max", "waiting", "value"),
	};
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);

	await choosingModel(page, "scripted-max");
	const refusal = page
		.getByRole("form", { name: "Session composer" })
		.getByRole("alert")
		.filter({ hasText: "the harness refused scripted-max" });
	await expect(refusal).toBeVisible();
	await expect(modelChoice(page)).toHaveText("scripted-mini");
	await expect(modelChoice(page)).toBeEnabled();

	scripted.session = sessionRead({
		options: [option({ warns_cache: false })],
		changing_options: [
			{ option: "model", category: "model", value: "scripted-max", participant: "river" },
		],
	});
	await page.reload();
	await expect(page.locator("[data-changing-option]")).toContainText(
		"river is changing model to scripted-max",
	);
	await expect(modelChoice(page)).toBeDisabled();
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

test("Shift+Tab warns before a mode that drops the cache, and announces it only once applied", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		unfinished_session: { id: SESSION_ID, name: "calm-river", state: "waiting", preparing: null },
	});
	scripted.session = sessionRead({ options: [modeOption(true)] });
	scripted.option = { status: 202, body: sessionRead({}) };
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);
	const writes = () => scripted.requests.filter((sent) => sent.path.endsWith("/options"));

	const input = page.getByLabel("Post");
	await input.focus();
	await input.press("Shift+Tab");

	const announcement = page.locator("[data-announcement]");
	await expect(announcement).toContainText("1,200 tokens");
	const confirm = page.locator("[data-option-confirm]");
	await expect(confirm).toContainText("Change Mode to plan?");
	expect(writes()).toHaveLength(0);
	await confirm.getByRole("button", { name: "Change" }).click();
	await expect(announcement).toHaveText("mode plan");
	expect(writes()).toHaveLength(1);

	scripted.session = sessionRead({ options: [modeOption(false)] });
	scripted.requests = [];
	await page.reload();
	const gate = Promise.withResolvers<void>();
	scripted.optionGate = gate.promise;
	await input.focus();
	await input.press("Shift+Tab");
	await expect.poll(() => writes().length).toBe(1);
	await input.press("Shift+Tab");
	await expect(announcement).not.toHaveText("mode plan");
	gate.resolve();
	await expect(announcement).toHaveText("mode plan");
	expect(writes()).toHaveLength(1);
});

test("slash commands filter actual offers, select by keyboard and send supported commands verbatim", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.session = sessionRead({
		commands: [
			{
				name: "compact",
				description: "Compact the conversation",
				input_hint: "/compact [instructions]",
			},
			{ name: "review", description: "Review changes", input_hint: "/review [path]" },
		],
	});
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);
	const input = page.getByRole("textbox", { name: "Post", exact: true });
	await input.fill("/rev");
	const commands = page.getByRole("listbox", { name: "Harness commands" });
	await expect(commands.getByRole("option")).toHaveCount(1);
	await expect(commands.getByRole("option")).toContainText("/review [path]");
	const axe = await new AxeBuilder({ page }).include("[data-composer]").analyze();
	expect(
		axe.violations.filter(({ impact }) => impact === "serious" || impact === "critical"),
	).toEqual([]);
	await input.press("ArrowDown");
	await input.press("Enter");
	await expect(input).toHaveValue("/review ");
	await expect(input).toBeFocused();
	await expect(commands).toHaveCount(0);
	await input.fill("/review  src/  ");
	await page.getByRole("button", { name: "Post", exact: true }).click();
	expect(scripted.requests.at(-1)?.body).toEqual({
		participant: "jack",
		message: "/review  src/  ",
	});
	await expect(input).toHaveValue("");
	// "changes" is in a description and "path" in a hint, and neither names a command.
	await input.fill("/changes");
	await expect(commands).toHaveCount(0);
	await input.press("Enter");
	await expect(input).toHaveValue("");
	expect(scripted.requests.at(-1)?.body).toEqual({ participant: "jack", message: "/changes" });
	await input.fill("/unsupported ordinary text");
	await expect(commands).toHaveCount(0);
	await page.getByRole("button", { name: "Post", exact: true }).click();
	expect(scripted.requests.at(-1)?.body).toEqual({
		participant: "jack",
		message: "/unsupported ordinary text",
	});
});

test("Session controls and actual context belong to the composer footer, leaving a quiet header", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 11,
				participant: "jack",
				message: "Please include the keyboard path in the review.",
				posted_at: "2026-10-01T10:00:00Z",
				edited_at: null,
			},
			{
				id: 12,
				participant: "river",
				message: "Check the narrow layout too.",
				posted_at: "2026-10-01T10:00:00Z",
				edited_at: null,
			},
		],
	});
	scripted.session = sessionRead({
		options: [option({ warns_cache: false }), modeOption(false)],
		usage: { context_used: 1200, context_size: 200000, cost: { amount: 0.42, currency: "USD" } },
	});
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);
	const composer = page.getByRole("form", { name: "Session composer" });
	await expect(composer.getByRole("button", { name: "Model: scripted-mini" })).toBeVisible();
	await expect(composer.getByRole("button", { name: /Context: 0.6%/ })).toBeVisible();
	await composer.getByRole("button", { name: /Context: 0.6%/ }).click();
	await expect(page.getByText("1,200 of 200,000 tokens", { exact: true })).toBeVisible();
	await expect(page.getByText("0.42 USD", { exact: true })).toBeVisible();
	await expect(composer.getByRole("button", { name: "Interrupt", exact: true })).toBeDisabled();
	await expect(page.locator("[data-session-header]").getByRole("button")).toHaveCount(0);
	await expect(page.locator("[data-session-header]")).not.toContainText("/compact");
	await page.setViewportSize({ width: 375, height: 800 });
	await expect(composer.getByRole("button", { name: "Model: scripted-mini" })).toBeVisible();
	await expect
		.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth))
		.toBe(true);
});

test("authors can amend and withdraw Held Messages before the Brief is ready", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, false);
	const scripted = new Scripted(workspace);
	scripted.session = sessionRead({ state: "unbriefed", options: [option({ warns_cache: false })] });
	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 7,
				participant: "jack",
				message: "my held message",
				posted_at: new Date().toISOString(),
				edited_at: null,
			},
			{
				id: 8,
				participant: "river",
				message: "another held message",
				posted_at: new Date().toISOString(),
				edited_at: null,
			},
		],
	});
	await scripted.install(page);
	await identified(page.context(), "jack");
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace}`);
	await expect(
		page
			.getByRole("form", { name: "Brief composer" })
			.getByRole("button", { name: "Model: scripted-mini" }),
	).toBeVisible();
	const held = page.getByRole("list", { name: "Held Messages" });
	const mine = held.getByRole("listitem").filter({ hasText: "my held message" });
	await expect(
		held.getByRole("listitem").filter({ hasText: "another held message" }).getByRole("button"),
	).toHaveCount(0);
	await mine.getByRole("button", { name: "Edit" }).click();
	await expect(page.getByRole("textbox", { name: "Edit the held message" })).toBeFocused();
	await page.getByRole("textbox", { name: "Edit the held message" }).fill("edited before ready");
	scripted.workspace = workspaceRead(workspace, {
		held_messages: [
			{
				id: 7,
				participant: "jack",
				message: "edited before ready",
				posted_at: new Date().toISOString(),
				edited_at: new Date().toISOString(),
			},
		],
	});
	await page.getByRole("button", { name: "Save", exact: true }).click();
	await expect(held.getByText("edited before ready", { exact: true })).toBeVisible();
	await expect(held.getByRole("button", { name: "Withdraw" })).toBeEnabled();
	scripted.workspace = workspaceRead(workspace);
	await held.getByRole("button", { name: "Withdraw" }).click();
	await expect(held).toHaveCount(0);
	await expect(page.getByRole("textbox", { name: "Message", exact: true })).toBeFocused();
	expect(scripted.requests).toContainEqual({
		method: "DELETE",
		path: `/operator/organizations/${ORGANIZATION}/workspaces/${workspace}/messages/7`,
		body: { participant: "jack" },
	});
});

test("Interrupt exposes pending and refused states and stays unavailable outside a working Turn", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.session = sessionRead({ state: "working" });
	const gate = Promise.withResolvers<void>();
	scripted.interruptGate = gate.promise;
	scripted.interrupt = {
		status: 409,
		body: conflict("The Turn could not be interrupted", "working"),
	};
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);
	await page.getByRole("button", { name: "Interrupt", exact: true }).click();
	await expect(page.getByRole("button", { name: "Interrupting…", exact: true })).toBeDisabled();
	gate.resolve();
	await expect(
		page.getByRole("alert").filter({ hasText: "The Turn could not be interrupted" }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "Interrupt", exact: true })).toBeEnabled();
	scripted.session = sessionRead({
		state: "working",
		interrupting: { participant: "river", requested_at: new Date().toISOString() },
	});
	await page.reload();
	await expect(page.getByRole("button", { name: "Interrupting…", exact: true })).toBeDisabled();
	const unavailable = async (state: string) => {
		scripted.session = sessionRead({ state, options: [option({ warns_cache: false })] });
		await page.reload();
		await expect(page.getByRole("button", { name: "Interrupt", exact: true })).toBeDisabled();
		if (state === "ended" || state === "unreachable")
			await expect(modelChoice(page)).toBeDisabled();
	};
	await unavailable("waiting");
	await unavailable("trailing");
	await unavailable("ended");
	await unavailable("unreachable");

	scripted.workspace = workspaceRead(workspace, { state: "sealed" });
	await page.reload();
	await expect(page.getByRole("form", { name: "Session composer" })).toHaveCount(0);
});

test("thought-level choices in the footer retain the offered values and option-write semantics", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted(workspace);
	scripted.session = sessionRead({
		options: [
			option({
				id: "reasoning",
				name: "Thought level",
				category: "thought_level",
				current: "low",
				values: [
					{ value: "low", name: "Low", description: null },
					{ value: "high", name: "High", description: null },
				],
				warns_cache: false,
			}),
		],
	});
	await scripted.install(page);
	await identified(page.context(), "jack");
	await visiting(page, workspace);
	await page
		.getByRole("form", { name: "Session composer" })
		.getByRole("button", { name: "High", exact: true })
		.click();
	expect(scripted.requests).toContainEqual({
		method: "POST",
		path: `/operator/organizations/${ORGANIZATION}/sessions/${SESSION_ID}/options`,
		body: { participant: "jack", category: "thought_level", value: "high" },
	});
});
