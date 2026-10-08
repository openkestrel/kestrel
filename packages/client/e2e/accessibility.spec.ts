import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";

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

async function noSeriousViolation(page: Page): Promise<void> {
	const { violations } = await new AxeBuilder({ page }).analyze();
	const serious = violations.filter(({ impact }) => impact === "serious" || impact === "critical");
	expect(
		serious
			.map(
				({ id, help, nodes }) =>
					`${id}: ${help} (${nodes.length} node(s))\n${nodes
						.map((node) => `  ${node.target.join(" ")} :: ${node.html.slice(0, 160)}`)
						.join("\n")}`,
			)
			.join("\n"),
	).toBe("");
}

test("the Workspaces list has no serious violation at desktop and 375 px", async ({
	page,
	request,
}) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);

	await page.goto("/organizations/acme");
	await expect(page.getByRole("heading", { name: "Workspaces" })).toBeVisible();
	await noSeriousViolation(page);

	await page.setViewportSize({ width: 375, height: 667 });
	await expect(page.getByRole("tab", { name: "Workspaces" })).toBeVisible();
	await noSeriousViolation(page);
});

test("every Work pane view has no serious violation", async ({ page, request }) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await opened.json()) as { workspace: { name: string } };

	await page.goto(`/organizations/acme/workspaces/${body.workspace.name}`);
	await expect(page.getByRole("heading", { name: body.workspace.name })).toBeVisible();
	await noSeriousViolation(page);

	const work = page.getByRole("region", { name: "Work", exact: true });
	for (const tab of ["Diff", "Files", "Sessions", "People"]) {
		// oxlint-disable-next-line no-await-in-loop -- each view is audited after its tab is chosen.
		await auditView(work, page, tab);
	}
});

async function auditView(work: Locator, page: Page, tab: string): Promise<void> {
	await work.getByRole("tab", { name: tab }).click();
	await noSeriousViolation(page);
}

test("the Transcript announces shared-state entries and restrains tool chatter", async ({
	page,
	request,
}) => {
	const opened = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(opened.ok(), await opened.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await opened.json()) as { workspace: { name: string } };

	await page.goto(`/organizations/acme/workspaces/${body.workspace.name}`);
	await expect(page.getByRole("heading", { name: body.workspace.name })).toBeVisible();

	await expect(page.getByRole("log")).toHaveAttribute("aria-live", "off");
	const announcer = page.locator("[data-transcript-announcement]");
	await expect(announcer).toHaveAttribute("aria-live", "polite");
	await expect(announcer).toHaveClass(/sr-only/);
});

test("the New Workspace form has no serious violation", async ({ page }) => {
	await page.goto("/organizations/acme/new");
	await expect(page.getByRole("heading", { name: "New Workspace" })).toBeVisible();
	await noSeriousViolation(page);

	await page.setViewportSize({ width: 375, height: 667 });
	await noSeriousViolation(page);
});

type WireEvent = { name: string; id?: string; data: unknown };

function wire(...events: WireEvent[]): string {
	return events
		.map(
			({ name, id, data }) =>
				`${id ? `id: ${id}\n` : ""}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`,
		)
		.join("");
}

const LINES = 100;

function lines(prefix: string): string[] {
	return Array.from({ length: LINES }, (_, index) => `${prefix} line ${index}`);
}

const session = "00000000-0000-0000-0000-0000000000ff";

async function openWorkspace(request: APIRequestContext): Promise<{ id: string; name: string }> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return { id: body.workspace.id, name: body.workspace.name };
}

function said(workspace: string, seq: number, message: unknown): WireEvent {
	return {
		name: "entry",
		id: `${workspace}:${seq}`,
		data: {
			kind: "shared_state",
			session_id: null,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: { type: "said", participant: "jack", message },
		},
	};
}

// A long Transcript with two tool calls whose input and result overflow, plus a payload reference
// the reader loads on demand.
async function populated(page: Page, request: APIRequestContext): Promise<void> {
	const { id, name } = await openWorkspace(request);
	const payload = lines("payload").join("\n");
	const reference = {
		payload_id: `${id}:63:message`,
		bytes: payload.length,
		media_type: "text/plain; charset=utf-8",
	};
	const tool = (seq: number): WireEvent => ({
		name: "entry",
		id: `${id}:${seq}`,
		data: {
			kind: "detail",
			session_id: session,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: {
				type: "tool_call",
				session_id: session,
				call_id: `call-${seq}`,
				title: `tool ${seq}`,
				tool_kind: "execute",
				status: "completed",
				input: { lines: lines("input") },
				result: { content: [], output: { lines: lines("output"), exit_code: 0 } },
				closing_reason: null,
				completion: {
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:00:01Z",
					turn_outcome: null,
				},
			},
		},
	});

	await page.route(
		(url) => url.pathname.endsWith("/transcript"),
		async (route) => {
			const url = new URL(route.request().url());
			if (url.searchParams.has("first_seq") && url.searchParams.has("last_seq")) {
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: wire(tool(61), tool(62), { name: "end", data: { because: "caught_up" } }),
				});
				return;
			}
			await route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: wire(
					...Array.from({ length: 60 }, (_, index) => said(id, index + 1, `message ${index + 1}`)),
					{
						name: "activity",
						id: `${id}:62`,
						data: {
							first_seq: 61,
							last_seq: 62,
							counts: { tool_calls: 2, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
							latest: { kind: "detail", title: "tool 62", status: "completed" },
							started_at: "2026-09-30T10:00:00Z",
							finished_at: "2026-09-30T10:00:02Z",
							anomaly: false,
							closed: true,
						},
					},
					{
						name: "entry",
						id: `${id}:63`,
						data: {
							kind: "shared_state",
							session_id: null,
							seq: 63,
							appended_at: "2026-09-30T10:00:03Z",
							entry: {
								type: "said",
								participant: "jack",
								message: reference,
								payload_fields: ["message"],
							},
						},
					},
					{ name: "end", data: { because: "sealed" } },
				),
			});
		},
	);
	await page.route(
		(url) => url.pathname.includes("/transcript/payloads/"),
		(route) =>
			route.fulfill({ status: 200, contentType: "text/plain; charset=utf-8", body: payload }),
	);

	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("heading", { name })).toBeVisible();
	await expect(page.getByRole("log").getByText("message 60")).toBeVisible();
}

async function scrollTranscriptUp(page: Page): Promise<void> {
	// The wheel lands on whatever is under the cursor, including a nested payload region that
	// consumes it; scroll the Transcript's own container instead.
	const scroller = page.getByRole("log").locator(":scope > div");
	await scroller.evaluate((element) => {
		element.scrollTop = 0;
	});
}

async function scrollsByKeyboard(region: Locator): Promise<void> {
	await region.evaluate((element) => {
		element.scrollTop = 0;
	});
	expect(await region.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(
		true,
	);
	await region.press("PageDown");
	await expect.poll(() => region.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
}

test("a long Transcript scrolled away from the bottom names its scroll-to-bottom control", async ({
	page,
	request,
}) => {
	await populated(page, request);

	const button = page.getByRole("button", { name: "Scroll to bottom" });
	await expect(button).toBeHidden();

	await scrollTranscriptUp(page);
	await expect(button).toBeVisible();
	await expect(button).toHaveAccessibleName("Scroll to bottom");

	await button.click();
	await expect(button).toBeHidden();
});

test("a Transcript of plain messages scrolls by keyboard and has no serious violation", async ({
	page,
	request,
}) => {
	const { id, name } = await openWorkspace(request);
	await page.route(
		(url) => url.pathname.endsWith("/transcript"),
		(route) =>
			route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: wire(
					...Array.from({ length: 60 }, (_, index) => said(id, index + 1, `message ${index + 1}`)),
					{ name: "end", data: { because: "sealed" } },
				),
			}),
	);

	await page.goto(`/organizations/acme/workspaces/${name}`);
	await expect(page.getByRole("log").getByText("message 60")).toBeVisible();
	await noSeriousViolation(page);
	await scrollsByKeyboard(page.getByRole("log").locator(":scope > div"));
});

test("expanded tool payloads scroll by keyboard and the populated Transcript has no serious violation", async ({
	page,
	request,
}) => {
	await populated(page, request);

	await page.getByRole("button", { name: "Steps" }).click();
	const activity = page.locator('[data-activity="61"]');
	const call = activity.locator('[data-seq="61"]');
	await expect(call).toBeVisible();
	await call.getByRole("button", { name: "More" }).click();
	const input = call.locator("pre").first();
	await expect(input).toContainText("input line 0");
	await scrollsByKeyboard(input);

	await page.getByRole("button", { name: "Full" }).click();
	const result = call.locator("pre").nth(1);
	await expect(result).toContainText("output line 0");
	await scrollsByKeyboard(result);

	const answered = page.locator('[data-seq="63"]');
	await answered.getByRole("button", { name: /Load .* payload/ }).click();
	const loaded = answered.locator("pre");
	await expect(loaded).toContainText("payload line 0");
	await scrollsByKeyboard(loaded);

	await scrollTranscriptUp(page);
	await expect(page.getByRole("button", { name: "Scroll to bottom" })).toBeVisible();
	await noSeriousViolation(page);
});
