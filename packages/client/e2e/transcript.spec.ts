import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { continuedTranscript, FIRST_SESSION, wire } from "./continued-workspace";
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
	const body = (await response.json()) as { workspace: Workspace };
	return body.workspace;
}

async function reading(page: Page, request: APIRequestContext): Promise<void> {
	const workspace = await opened(request);
	await new Streamed({ transcript: () => continuedTranscript(workspace.id) }).install(page);
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
	await expect(page.getByRole("log").getByText("Session succeeded")).toBeVisible();
}

test("each message is attributed to its Participant, in the Transcript's order", async ({
	page,
	request,
}) => {
	await reading(page, request);
	const log = page.getByRole("log");

	await expect(log.getByRole("article", { name: "priya" })).toHaveText([
		/Keep the cap under a minute/,
		/And add a test for the cap\./,
	]);
	await expect(log.getByRole("article", { name: "jack" })).toHaveText([
		/Try again, and keep reserve\(\) as it is\./,
	]);
	await expect(log.getByRole("article", { name: "builder" })).toHaveText([
		/I'll start with tab-stream\.ts/,
		/Backoff for the follow/,
	]);

	const read = await log.innerText();
	const positions = [
		"jack joined",
		"builder started",
		"I'll start with",
		"priya joined",
		"Keep the cap",
		"Session failed",
		"Try again",
		"And add a test",
		"Backoff for the follow",
		"Pull request 705",
		"Session succeeded",
	].map((text) => read.indexOf(text));
	expect(positions).not.toContain(-1);
	expect(positions).toEqual(positions.toSorted((one, other) => one - other));
});

test("an agent's Markdown reads as a heading, emphasis, code and a table", async ({
	page,
	request,
}) => {
	await reading(page, request);
	const answer = page.getByRole("log").getByRole("article", { name: "builder" }).last();

	await expect(answer.getByRole("heading", { name: "Backoff for the follow" })).toBeVisible();
	await expect(answer.getByRole("strong")).toHaveText("doubles");
	await expect(answer.getByRole("emphasis")).toHaveText("30 seconds");
	await expect(answer.getByRole("code").filter({ hasText: "reserve()" })).toBeVisible();
	await expect(answer.getByText("export function wait(attempt: number): number {")).toBeVisible();
	await expect(answer.getByRole("columnheader", { name: "Attempt" })).toBeVisible();
	await expect(answer.getByRole("cell", { name: "30s" })).toBeVisible();
	await expect(answer.getByRole("checkbox")).toHaveCount(2);
	await expect(answer.getByRole("checkbox").first()).toBeChecked();
	await expect(answer).not.toContainText("**doubles**");
	await expect(answer).not.toContainText("```");
	await expect(answer).not.toContainText("| ---");

	const person = page.getByRole("log").getByRole("article", { name: "priya" }).first();
	await expect(person.getByRole("emphasis")).toHaveText("under");
});

test("a Session-ended row says how the Session went, and why it failed", async ({
	page,
	request,
}) => {
	await reading(page, request);
	const log = page.getByRole("log");

	await expect(
		log.getByText("Session failed: the harness exited with status 1", { exact: true }),
	).toBeVisible();
	await expect(log.getByText("Session succeeded", { exact: true })).toBeVisible();
	await expect(log.getByText("Session ended")).toHaveCount(0);
});

test("shared-state entries, a tombstone and the disclosure control keep their place", async ({
	page,
	request,
}) => {
	await reading(page, request);
	const log = page.getByRole("log");

	await expect(log.getByText("jack joined", { exact: true })).toBeVisible();
	await expect(log.getByRole("strong").filter({ hasText: "back off" })).toBeVisible();
	await expect(log.getByText("builder started", { exact: true })).toHaveCount(2);
	await expect(log.getByText(/^expired /)).toBeVisible();
	await expect(
		log.getByRole("link", { name: "Pull request 705: Back off between Transcript follow retries" }),
	).toHaveAttribute("href", "https://github.com/openkestrel/kestrel/pull/705");

	const full = page.getByRole("button", { name: "Full", exact: true });
	await full.click();
	await expect(full).toHaveAttribute("aria-pressed", "true");
	await expect(log.getByRole("article", { name: "builder" })).toHaveCount(2);
});

test("the Transcript is reachable by keyboard and has no serious violation", async ({
	page,
	request,
}) => {
	await reading(page, request);

	await page.getByRole("button", { name: "Full", exact: true }).focus();
	await page.keyboard.press("Tab");
	await expect(page.getByRole("log").locator(":scope > div")).toBeFocused();

	for (const width of [1280, 375]) {
		// oxlint-disable-next-line no-await-in-loop -- each width is audited after it is set.
		await page.setViewportSize({ width, height: 800 });
		// oxlint-disable-next-line no-await-in-loop -- each width is audited after it is set.
		const { violations } = await new AxeBuilder({ page }).analyze();
		expect(
			violations
				.filter(({ impact }) => impact === "serious" || impact === "critical")
				.map(({ id, nodes }) => `${id}: ${nodes.map((node) => node.html.slice(0, 160)).join("\n")}`)
				.join("\n"),
		).toBe("");
		// oxlint-disable-next-line no-await-in-loop -- each width is measured after it is set.
		const overflows = await page.evaluate(
			() => document.documentElement.scrollWidth > document.documentElement.clientWidth,
		);
		expect(overflows).toBe(false);
	}
});

test("a thought opened inside an Activity renders its Markdown", async ({ page, request }) => {
	const workspace = await opened(request);
	const thought = {
		name: "entry",
		id: `${workspace.id}:2`,
		data: {
			kind: "narration",
			session_id: FIRST_SESSION,
			seq: 2,
			appended_at: "2026-09-30T10:00:00Z",
			entry: {
				type: "thought",
				session_id: FIRST_SESSION,
				text: "The cap belongs in `wait()`.\n\nIt must **never** exceed the outage page's.",
				completion: {
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:00:01Z",
					turn_outcome: null,
				},
			},
		},
	};
	await page.route(
		(url) => url.pathname.endsWith("/transcript"),
		(route) =>
			route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: wire(thought, { name: "end", data: { because: "caught_up" } }),
			}),
	);
	await new Streamed({
		transcript: () =>
			wire(
				{
					name: "activity",
					id: `${workspace.id}:2`,
					data: {
						first_seq: 2,
						last_seq: 2,
						counts: { tool_calls: 0, failed_calls: 0, thoughts: 1, plans: 0, tombstones: 0 },
						latest: null,
						started_at: null,
						finished_at: null,
						anomaly: false,
						closed: true,
					},
				},
				{ name: "end", data: { because: "sealed" } },
			),
	}).install(page);
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);

	await page.getByRole("button", { name: "Full", exact: true }).click();
	const log = page.getByRole("log");
	await expect(log.getByRole("strong")).toHaveText("never");
	await expect(log.getByRole("code")).toHaveText("wait()");
	await expect(log.getByRole("button", { name: "Less" })).toHaveAttribute("aria-expanded", "true");
});
