import { expect, test, type APIRequestContext, type Page, type Request } from "@playwright/test";

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

type Workspace = { id: string; name: string };

async function opened(request: APIRequestContext): Promise<Workspace> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return { id: body.workspace.id, name: body.workspace.name };
}

type WireEvent = { name: string; id?: string; data: unknown };

function wire(...events: WireEvent[]): string {
	return events
		.map(
			({ name, id, data }) =>
				`${id ? `id: ${id}\n` : ""}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`,
		)
		.join("");
}

function entry(workspace: Workspace, seq: number, value: unknown): WireEvent {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "shared_state",
			session_id: null,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: value,
		},
	};
}

function summary(
	workspace: Workspace,
	first: number,
	last: number,
	overrides: Record<string, unknown> = {},
): WireEvent {
	return {
		name: "activity",
		id: `${workspace.id}:${last}`,
		data: {
			first_seq: first,
			last_seq: last,
			counts: { tool_calls: 0, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
			latest: null,
			started_at: null,
			finished_at: null,
			anomaly: false,
			closed: true,
			...overrides,
		},
	};
}

function tool(workspace: Workspace, seq: number, overrides: Record<string, unknown> = {}) {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "detail",
			session_id: "00000000-0000-0000-0000-0000000000ff",
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: {
				type: "tool_call",
				session_id: "00000000-0000-0000-0000-0000000000ff",
				call_id: `call-${seq}`,
				title: `tool ${seq}`,
				tool_kind: "execute",
				status: "completed",
				input: {},
				result: { content: [], output: {} },
				closing_reason: null,
				completion: {
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:00:01Z",
					turn_outcome: null,
				},
				...overrides,
			},
		},
	};
}

class Scripted {
	readonly requests: Request[] = [];
	readonly ranges: { first: number; last: number; kinds: string | null }[] = [];
	readonly payloads: string[] = [];

	private readonly follow: (attempt: number) => string | Promise<string>;
	private readonly range: (first: number, last: number) => string;
	private readonly payload: (id: string) => { status: number; body?: string };

	constructor(options: {
		follow: (attempt: number) => string | Promise<string>;
		range?: (first: number, last: number) => string;
		payload?: (id: string) => { status: number; body?: string };
	}) {
		this.follow = options.follow;
		this.range = options.range ?? (() => "");
		this.payload = options.payload ?? (() => ({ status: 404 }));
	}

	async install(page: Page): Promise<void> {
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			async (route) => {
				const url = new URL(route.request().url());
				this.requests.push(route.request());
				const first = url.searchParams.get("first_seq");
				const last = url.searchParams.get("last_seq");
				if (first && last) {
					this.ranges.push({
						first: Number(first),
						last: Number(last),
						kinds: url.searchParams.get("kinds"),
					});
					await route.fulfill({
						status: 200,
						contentType: "text/event-stream",
						body: this.range(Number(first), Number(last)),
					});
					return;
				}
				const attempts = this.requests.filter((request) =>
					new URL(request.url()).searchParams.has("follow"),
				).length;
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: await this.follow(attempts),
				});
			},
		);
		await page.route(
			(url) => url.pathname.includes("/transcript/payloads/"),
			async (route) => {
				const id = decodeURIComponent(
					new URL(route.request().url()).pathname.split("/").at(-1) ?? "",
				);
				this.payloads.push(id);
				const answer = this.payload(id);
				if (answer.status === 200) {
					await route.fulfill({
						status: 200,
						contentType: "text/plain; charset=utf-8",
						body: answer.body ?? "",
					});
					return;
				}
				await route.fulfill({
					status: answer.status,
					contentType: "application/json",
					body: JSON.stringify({ message: "the Transcript payload has expired" }),
				});
			},
		);
	}
}

async function visiting(page: Page, workspace: Workspace): Promise<void> {
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
}

test("a late joiner reads one line per Activity and expands only the one it opens", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const tools = Array.from({ length: 250 }, (_, index) => tool(workspace, index + 2));
	const scripted = new Scripted({
		follow: () =>
			wire(
				entry(workspace, 1, { type: "participant_joined", participant: "builder" }),
				summary(workspace, 2, 251, {
					counts: { tool_calls: 250, failed_calls: 1, thoughts: 20, plans: 5, tombstones: 2 },
					latest: { kind: "detail", title: "cargo test", status: "completed" },
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:05:00Z",
					anomaly: true,
				}),
				entry(workspace, 252, { type: "said", participant: "jack", message: "all done" }),
				summary(workspace, 253, 260, { counts: { thoughts: 3 } }),
				entry(workspace, 261, { type: "said", participant: "jack", message: "later" }),
				{ name: "end", data: { because: "sealed" } },
			),
		range: () => wire(...tools, { name: "end", data: { because: "caught_up" } }),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const first = page.locator('[data-activity="2"]');
	await expect(first).toContainText("Activity 2–251");
	await expect(first).toContainText("250 tools, 1 failed, 20 thoughts, 5 plans, 2 expired");
	await expect(first).toContainText("interrupted or unresolved");
	await expect(first).toContainText("cargo test · completed");
	await expect(first.locator("[data-seq]")).toHaveCount(0);
	await expect(page.getByRole("log").getByText("jack: all done")).toBeVisible();
	await expect(page.getByRole("log").getByText("jack: later")).toBeVisible();
	await expect(page.locator('[data-activity="253"] [data-seq]')).toHaveCount(0);

	await first.getByRole("button").first().click();

	await expect(first.locator("[data-seq]")).toHaveCount(250);
	await expect(page.locator('[data-activity="253"] [data-seq]')).toHaveCount(0);
	expect(scripted.ranges).toEqual([
		{ first: 2, last: 251, kinds: "shared_state,narration,detail" },
	]);
});

test("a running tool shows in the live line, and its completed call shows status, timing and exit", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const session = "00000000-0000-0000-0000-0000000000ff";
	const state = (tools: unknown[]) => ({
		session_id: session,
		tools,
		message_buffering: false,
		thought_buffering: false,
	});
	const held = Promise.withResolvers<void>();
	const scripted = new Scripted({
		follow: (attempt) => {
			if (attempt === 1) {
				return wire({
					name: "session_state",
					data: state([
						{
							call_id: "call-1",
							title: "cargo test",
							status: "in_progress",
							started_at: "2026-09-30T10:00:00Z",
						},
					]),
				});
			}
			return held.promise.then(() =>
				wire(
					summary(workspace, 1, 1, {
						counts: { tool_calls: 1 },
						latest: { kind: "detail", title: "cargo test", status: "completed" },
						started_at: "2026-09-30T10:00:00Z",
						finished_at: "2026-09-30T10:00:02.400Z",
					}),
					{ name: "session_state", data: state([]) },
					{ name: "end", data: { because: "sealed" } },
				),
			);
		},
		range: () =>
			wire(
				tool(workspace, 1, {
					title: "cargo test",
					status: "completed",
					completion: {
						started_at: "2026-09-30T10:00:00Z",
						finished_at: "2026-09-30T10:00:02.400Z",
						turn_outcome: null,
					},
					result: { content: [], output: { exit_code: 0 } },
				}),
				{ name: "end", data: { because: "caught_up" } },
			),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	await expect(page.locator("[data-live]")).toContainText("cargo test · in_progress");
	held.resolve();

	await page.getByRole("button", { name: "Steps" }).click();

	const activity = page.locator('[data-activity="1"]');
	const row = activity.locator('[data-seq="1"]');
	await expect(row).toContainText("cargo test");
	await expect(row).toContainText("completed");
	await expect(row).toContainText("2.4s");
	await expect(row).toContainText("exit 0");
	await expect(page.locator("[data-live]")).toHaveCount(0);
});

test("a failed, an interrupted and an unresolved call each show their word", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: () =>
			wire(
				summary(workspace, 1, 3, {
					counts: { tool_calls: 3, failed_calls: 1 },
					latest: { kind: "detail", title: "tool 3", status: "pending" },
					anomaly: true,
				}),
				{ name: "end", data: { because: "sealed" } },
			),
		range: () =>
			wire(
				tool(workspace, 1, { status: "failed", result: { content: [], output: { exit_code: 1 } } }),
				tool(workspace, 2, { status: "in_progress", closing_reason: "interrupted" }),
				tool(workspace, 3, { status: "pending", closing_reason: "unresolved" }),
				{ name: "end", data: { because: "caught_up" } },
			),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	await page.getByRole("button", { name: "Full" }).click();

	const activity = page.locator('[data-activity="1"]');
	await expect(activity.locator('[data-seq="1"]')).toContainText("failed");
	await expect(activity.locator('[data-seq="1"]')).toContainText("exit 1");
	await expect(activity.locator('[data-seq="2"]')).toContainText("interrupted");
	await expect(activity.locator('[data-seq="3"]')).toContainText("unresolved");
});

test("an Activity is replaced as it grows, and a reconnect adds no duplicate entries", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: (attempt) =>
			attempt === 1
				? wire(
						entry(workspace, 1, { type: "said", participant: "jack", message: "hello" }),
						summary(workspace, 2, 3, {
							counts: { tool_calls: 1 },
							closed: false,
							latest: { kind: "detail", title: "tool 2", status: "in_progress" },
						}),
					)
				: wire(
						summary(workspace, 2, 5, {
							counts: { tool_calls: 2 },
							latest: { kind: "detail", title: "tool 5", status: "completed" },
						}),
						entry(workspace, 6, { type: "said", participant: "jack", message: "done" }),
						{ name: "end", data: { because: "sealed" } },
					),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	await expect(page.getByRole("log").getByText("jack: done")).toBeVisible();
	await expect(page.locator('[data-activity="2"]')).toHaveCount(1);
	await expect(page.locator('[data-activity="2"]')).toContainText("2 tools");
	await expect(page.getByRole("log").getByText("jack: hello")).toHaveCount(1);
	await expect(page.getByRole("log").getByText("jack: done")).toHaveCount(1);

	const resumed = scripted.requests.filter((candidate) =>
		new URL(candidate.url()).searchParams.has("follow"),
	);
	expect(resumed).toHaveLength(2);
	expect(resumed[1]?.headers()["last-event-id"]).toBe(`${workspace.id}:3`);
});

test("a referenced payload loads on demand, and an expired one reads as expired", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const reference = (seq: number) => ({
		payload_id: `${workspace.id}:${seq}:message`,
		bytes: 70_000,
		media_type: "text/plain; charset=utf-8",
	});
	const scripted = new Scripted({
		follow: () =>
			wire(
				{
					name: "entry",
					id: `${workspace.id}:1`,
					data: {
						kind: "shared_state",
						session_id: null,
						seq: 1,
						appended_at: "2026-09-30T10:00:00Z",
						entry: {
							type: "said",
							participant: "jack",
							message: reference(1),
							payload_fields: ["message"],
						},
					},
				},
				{
					name: "entry",
					id: `${workspace.id}:2`,
					data: {
						kind: "shared_state",
						session_id: null,
						seq: 2,
						appended_at: "2026-09-30T10:00:01Z",
						entry: {
							type: "said",
							participant: "jack",
							message: reference(2),
							payload_fields: ["message"],
						},
					},
				},
				{ name: "end", data: { because: "sealed" } },
			),
		payload: (id) =>
			id.endsWith(":1:message")
				? { status: 200, body: "the whole story" }
				: { status: 410, body: "expired" },
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const first = page.locator('[data-seq="1"]').getByRole("button", { name: "Load 68 KiB payload" });
	const second = page
		.locator('[data-seq="2"]')
		.getByRole("button", { name: "Load 68 KiB payload" });
	await expect(first).toBeVisible();
	await expect(second).toBeVisible();

	await first.click();
	await expect(page.getByText("the whole story")).toBeVisible();

	await second.click();
	await expect(page.getByText("expired")).toBeVisible();
});
