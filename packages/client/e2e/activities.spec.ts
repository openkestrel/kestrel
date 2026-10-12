import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { Streamed } from "./streamed";

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

type RangeAnswer = string | { refused: number };

const SESSION = "00000000-0000-0000-0000-0000000000ff";
const COMPLETION = {
	started_at: "2026-09-30T10:00:00Z",
	finished_at: "2026-09-30T10:00:01Z",
	turn_outcome: null,
};

function narrated(workspace: Workspace, seq: number, value: Record<string, unknown>): WireEvent {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "narration",
			session_id: SESSION,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: { session_id: SESSION, completion: COMPLETION, ...value },
		},
	};
}

const caughtUp: WireEvent = { name: "end", data: { because: "caught_up" } };

function working(tools: unknown[], buffering: { thought?: boolean } = {}): WireEvent {
	return {
		name: "session_state",
		data: {
			session_id: SESSION,
			tools,
			units: [],
			message_buffering: false,
			thought_buffering: buffering.thought ?? false,
			usage: null,
			observation: { availability: "current" },
		},
	};
}

const stillOpen = () => new Promise<string>(() => {});

class Scripted {
	readonly ranges: { first: number; last: number; kinds: string | null }[] = [];
	readonly payloads: string[] = [];
	readonly streamed: Streamed;

	private readonly range: (first: number, last: number) => RangeAnswer | Promise<RangeAnswer>;
	private readonly payload: (id: string) => { status: number; body?: string };

	constructor(options: {
		follow: (attempt: number) => string | Promise<string>;
		range?: (first: number, last: number) => RangeAnswer | Promise<RangeAnswer>;
		payload?: (id: string) => { status: number; body?: string };
	}) {
		this.streamed = new Streamed({ transcript: (_, attempt) => options.follow(attempt) });
		this.range = options.range ?? (() => "");
		this.payload = options.payload ?? (() => ({ status: 404 }));
	}

	async install(page: Page): Promise<void> {
		await this.streamed.install(page);
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			async (route) => {
				const url = new URL(route.request().url());
				const first = url.searchParams.get("first_seq");
				const last = url.searchParams.get("last_seq");
				this.ranges.push({
					first: Number(first),
					last: Number(last),
					kinds: url.searchParams.get("kinds"),
				});
				const answer = await this.range(Number(first), Number(last));
				if (typeof answer !== "string") {
					await route.fulfill({
						status: answer.refused,
						contentType: "application/json",
						body: JSON.stringify({ message: "the Transcript cannot be read" }),
					});
					return;
				}
				await route.fulfill({ status: 200, contentType: "text/event-stream", body: answer });
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
				summary(workspace, 253, 260, {
					counts: { tool_calls: 0, failed_calls: 0, thoughts: 3, plans: 0, tombstones: 0 },
				}),
				entry(workspace, 261, { type: "said", participant: "jack", message: "later" }),
				{ name: "end", data: { because: "sealed" } },
			),
		range: () => wire(...tools, caughtUp),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	const first = log.getByRole("button", {
		name: "Activity, 250 tool calls, 1 failed, 20 thoughts, 5 plans, 2 expired, interrupted or unresolved, 5m 0s",
		exact: true,
	});
	const second = log.getByRole("button", { name: "Activity, 3 thoughts", exact: true });
	await expect(first).toHaveAttribute("aria-expanded", "false");
	await expect(second).toHaveAttribute("aria-expanded", "false");
	await expect(log.getByRole("listitem")).toHaveCount(0);
	await expect(
		log.getByRole("article", { name: "jack" }).filter({ hasText: "all done" }),
	).toBeVisible();
	await expect(
		log.getByRole("article", { name: "jack" }).filter({ hasText: "later" }),
	).toBeVisible();
	expect(scripted.ranges).toEqual([]);

	await first.focus();
	await page.keyboard.press("Enter");

	await expect(first).toHaveAttribute("aria-expanded", "true");
	await expect(first).toBeFocused();
	await expect(log.getByRole("list", { name: "Activity steps" }).getByRole("listitem")).toHaveCount(
		250,
	);
	await expect(second).toHaveAttribute("aria-expanded", "false");
	expect(scripted.ranges).toEqual([
		{ first: 2, last: 251, kinds: "shared_state,narration,detail" },
	]);
});

test("a running call is a row with nothing to open, and its completed call replaces it in place", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const held = Promise.withResolvers<void>();
	const scripted = new Scripted({
		follow: (attempt) => {
			if (attempt === 1) {
				return wire(
					working([
						{
							call_id: "call-1",
							title: "cargo test",
							tool_kind: "execute",
							status: "in_progress",
							started_at: "2026-09-30T10:00:00Z",
						},
					]),
				);
			}
			if (attempt > 2) return stillOpen();
			return held.promise.then(() =>
				wire(
					summary(workspace, 1, 1, {
						counts: { tool_calls: 1, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
						latest: { kind: "detail", title: "cargo test", status: "completed" },
						started_at: "2026-09-30T10:00:00Z",
						finished_at: "2026-09-30T10:00:02.400Z",
						closed: false,
					}),
					working([]),
				),
			);
		},
		range: () =>
			wire(
				tool(workspace, 1, {
					title: "cargo test",
					completion: { ...COMPLETION, finished_at: "2026-09-30T10:00:02.400Z" },
					result: {
						content: [{ type: "content", content: { type: "text", text: "test result: ok" } }],
						output: { exit_code: 0 },
					},
				}),
				caughtUp,
			),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await expect(log.getByRole("button", { name: "Activity, Working", exact: true })).toHaveAttribute(
		"aria-expanded",
		"true",
	);
	const rows = log.getByRole("list", { name: "Activity steps" }).getByRole("listitem");
	await expect(rows).toHaveCount(1);
	await expect(rows).toContainText("cargo test");
	await expect(rows).toContainText("running");
	await expect(rows.getByRole("button")).toHaveCount(0);
	expect(scripted.ranges).toEqual([]);
	const running = await rows.elementHandle();

	held.resolve();

	const call = rows.getByRole("button", {
		name: "cargo test, completed, exit 0, 2.4s",
		exact: true,
	});
	await expect(call).toHaveAttribute("aria-expanded", "false");
	await expect(rows).toHaveCount(1);
	expect(await running.evaluate((row) => row.isConnected)).toBe(true);
	await expect(
		log.getByRole("button", { name: "Activity, Working, 1 tool call", exact: true }),
	).toHaveAttribute("aria-expanded", "true");

	await call.focus();
	await page.keyboard.press("Enter");

	await expect(call).toHaveAttribute("aria-expanded", "true");
	await expect(call).toBeFocused();
	await expect(rows.getByRole("region", { name: "Output" })).toContainText("test result: ok");
});

test("a failed, an interrupted and an unresolved call each say so, and a failure opens to its error", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: () =>
			wire(
				summary(workspace, 1, 3, {
					counts: { tool_calls: 3, failed_calls: 1, thoughts: 0, plans: 0, tombstones: 0 },
					latest: { kind: "detail", title: "tool 3", status: "pending" },
					anomaly: true,
				}),
				{ name: "end", data: { because: "sealed" } },
			),
		range: () =>
			wire(
				tool(workspace, 1, {
					status: "failed",
					result: {
						content: [{ type: "content", content: { type: "text", text: "no such file" } }],
						output: { exit_code: 1 },
					},
				}),
				tool(workspace, 2, { status: "in_progress", closing_reason: "interrupted" }),
				tool(workspace, 3, { status: "pending", closing_reason: "unresolved" }),
				caughtUp,
			),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await log
		.getByRole("button", {
			name: "Activity, 3 tool calls, 1 failed, interrupted or unresolved",
			exact: true,
		})
		.click();

	const failed = log.getByRole("button", { name: "tool 1, failed, exit 1, 1.0s", exact: true });
	await expect(
		log.getByRole("button", { name: "tool 2, interrupted, 1.0s", exact: true }),
	).toBeVisible();
	await expect(
		log.getByRole("button", { name: "tool 3, unresolved, 1.0s", exact: true }),
	).toBeVisible();

	await failed.click();
	await expect(log.getByRole("alert")).toHaveText("no such file");
});

test("thoughts are single lines that open to their Markdown, and a plan opens to its steps", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: () =>
			wire(
				summary(workspace, 1, 2, {
					counts: { tool_calls: 0, failed_calls: 0, thoughts: 1, plans: 1, tombstones: 0 },
				}),
				{ name: "end", data: { because: "sealed" } },
			),
		range: () =>
			wire(
				narrated(workspace, 1, {
					type: "thought",
					text: "The cap belongs in `wait()`.\n\nIt must **never** exceed the outage page's.",
				}),
				narrated(workspace, 2, {
					type: "plan",
					entries: [
						{ content: "Write the test", priority: "high", status: "completed" },
						{ content: "Cap the wait", priority: "high", status: "in_progress" },
					],
				}),
				caughtUp,
			),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await log.getByRole("button", { name: "Activity, 1 thought, 1 plan", exact: true }).click();

	const thought = log.getByRole("button", {
		name: "Thought, The cap belongs in wait()., 1.0s",
		exact: true,
	});
	await expect(thought).toHaveAttribute("aria-expanded", "false");
	await expect(log.getByRole("strong")).toHaveCount(0);
	await thought.focus();
	await page.keyboard.press("Space");
	await expect(thought).toHaveAttribute("aria-expanded", "true");
	await expect(log.getByRole("strong")).toHaveText("never");

	const plan = log.getByRole("button", { name: "Plan, 2 steps", exact: true });
	await plan.click();
	const steps = log.getByRole("list", { name: "Plan" }).getByRole("listitem");
	await expect(steps).toHaveText(["completed: Write the test", "in progress: Cap the wait"]);
});

test("a slow Activity read says it is delayed, and a failed one is retried from where it failed", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const slow = Promise.withResolvers<void>();
	let reads = 0;
	const scripted = new Scripted({
		follow: () =>
			wire(
				summary(workspace, 1, 2, {
					counts: { tool_calls: 2, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
				}),
				{ name: "end", data: { because: "sealed" } },
			),
		range: async () => {
			reads += 1;
			if (reads > 1) return wire(tool(workspace, 1), tool(workspace, 2), caughtUp);
			await slow.promise;
			return { refused: 409 };
		},
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	const activity = log.getByRole("button", { name: "Activity, 2 tool calls", exact: true });
	await activity.click();

	await expect(log.getByRole("status")).toHaveText(
		"Still reading the Activity; the Operator has not answered yet.",
	);
	await expect(log.getByText("This Activity has no entries.")).toHaveCount(0);

	slow.resolve();

	await expect(log.getByRole("alert")).toContainText("The Activity could not be read");
	const again = log.getByRole("button", { name: "Read the Activity again" });
	await again.focus();
	await page.keyboard.press("Enter");

	await expect(log.getByRole("list", { name: "Activity steps" }).getByRole("listitem")).toHaveCount(
		2,
	);
	await expect(log.getByRole("alert")).toHaveCount(0);
	await expect(activity).toBeFocused();
});

test("an Activity that grew keeps the rows it had when its next read fails", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const grown = Promise.withResolvers<void>();
	let refuse = true;
	const scripted = new Scripted({
		follow: (attempt) => {
			if (attempt === 1) {
				return wire(
					summary(workspace, 1, 1, {
						counts: { tool_calls: 1, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
						closed: false,
					}),
				);
			}
			if (attempt > 2) return stillOpen();
			return grown.promise.then(() =>
				wire(
					summary(workspace, 1, 2, {
						counts: { tool_calls: 2, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
						closed: false,
					}),
				),
			);
		},
		range: (_, last) => {
			if (last === 1) return wire(tool(workspace, 1), caughtUp);
			if (refuse) return { refused: 409 };
			return wire(tool(workspace, 1), tool(workspace, 2), caughtUp);
		},
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await log.getByRole("button", { name: "Activity, 1 tool call", exact: true }).click();
	const rows = log.getByRole("list", { name: "Activity steps" }).getByRole("listitem");
	await expect(rows).toHaveCount(1);

	grown.resolve();

	await expect(log.getByRole("alert")).toContainText("The Activity could not be read");
	await expect(rows).toHaveCount(1);
	await expect(rows.getByRole("button", { name: /^tool 1, completed/ })).toBeVisible();

	refuse = false;
	await log.getByRole("button", { name: "Read the Activity again" }).click();

	await expect(rows).toHaveCount(2);
	await expect(log.getByRole("alert")).toHaveCount(0);
});

test("an Activity whose read answers with nothing says it is empty", async ({ page, request }) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: () => wire(summary(workspace, 1, 1), { name: "end", data: { because: "sealed" } }),
		range: () => wire(caughtUp),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await log.getByRole("button", { name: "Activity", exact: true }).click();

	await expect(log.getByText("This Activity has no entries.")).toBeVisible();
	await expect(log.getByRole("alert")).toHaveCount(0);
});

test("the live Activity says the agent is thinking while a thought is buffering", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: (attempt) => (attempt === 1 ? wire(working([], { thought: true })) : stillOpen()),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await expect(log.getByRole("button", { name: "Activity, Working", exact: true })).toHaveAttribute(
		"aria-expanded",
		"true",
	);
	await expect(log.getByRole("listitem")).toHaveText(["Thinking…"]);
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
							counts: { tool_calls: 1, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
							closed: false,
							latest: { kind: "detail", title: "tool 2", status: "in_progress" },
						}),
					)
				: wire(
						summary(workspace, 2, 5, {
							counts: { tool_calls: 2, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
							latest: { kind: "detail", title: "tool 5", status: "completed" },
						}),
						entry(workspace, 6, { type: "said", participant: "jack", message: "done" }),
						{ name: "end", data: { because: "sealed" } },
					),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await expect(
		log.getByRole("article", { name: "jack" }).filter({ hasText: "done" }),
	).toBeVisible();
	await expect(log.getByRole("button", { name: /^Activity/ })).toHaveCount(1);
	await expect(
		log.getByRole("button", { name: "Activity, 2 tool calls", exact: true }),
	).toHaveAttribute("aria-expanded", "false");
	await expect(log.getByRole("article", { name: "jack" }).filter({ hasText: "hello" })).toHaveCount(
		1,
	);
	await expect(log.getByRole("article", { name: "jack" }).filter({ hasText: "done" })).toHaveCount(
		1,
	);

	const resumed = scripted.streamed.subscriptions.filter(
		(subscribed) => subscribed.kind === "transcript",
	);
	expect(resumed).toHaveLength(2);
	expect(resumed[0]?.after).toBeUndefined();
	expect(resumed[1]?.after).toBe(`${workspace.id}:3`);
	expect(resumed[1]?.token).not.toBe(resumed[0]?.token);
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
