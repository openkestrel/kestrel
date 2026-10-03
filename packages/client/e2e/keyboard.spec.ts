import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";

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

test.afterEach(async ({ page }) => {
	await page.unrouteAll({ behavior: "ignoreErrors" });
});

async function opened(request: APIRequestContext): Promise<{ id: string; name: string }> {
	const response = await request.post(`/operator/organizations/${ORGANIZATION}/workspaces`, {
		data: { project: "kestrel", agent: "builder", brief: "an opening brief" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return body.workspace;
}

async function tabTo(page: Page, target: Locator, what: string, tries = 80): Promise<void> {
	for (let index = 0; index < tries; index += 1) {
		// oxlint-disable-next-line no-await-in-loop -- a keyboard-only path steps one Tab at a time.
		await page.keyboard.press("Tab");
		// oxlint-disable-next-line no-await-in-loop -- checking where focus landed is the step.
		if (await target.evaluate((element) => element === document.activeElement)) return;
	}
	throw new Error(`the keyboard path never reached ${what}`);
}

function workingSession(workspace: { id: string }) {
	return [
		{
			id: "00000000-0000-0000-0000-000000000001",
			name: "calm-river-abcdefgh",
			workspace: workspace.id,
			state: "working",
			preparing: null,
			exit: null,
			outcome_message: null,
			instance: "local-1",
			supervisor: "1.0.0",
			agent: "builder",
			harness: "opencode",
			model: "claude-opus-5",
			mode: "code",
			thought_level: null,
			worked_model: "claude-opus-5",
			title: "a running conversation",
			options: [
				{
					id: "mode",
					name: "Mode",
					description: null,
					category: "mode",
					kind: "select",
					current: "code",
					values: [
						{ value: "code", name: "Code", description: null },
						{ value: "plan", name: "Plan", description: null },
					],
					groups: [],
					warns_cache: false,
				},
			],
			commands: [],
			enqueued_at: "2026-09-30T10:00:00Z",
			started_at: "2026-09-30T10:00:01Z",
			ended_at: null,
			lease_expires_at: null,
			connected_at: "2026-09-30T10:00:01Z",
			supervisor_version: "1.0.0",
			usage: null,
			changing_options: [],
			interrupting: null,
			tools: [
				{
					call_id: "call-1",
					title: "cargo test",
					status: "in_progress",
					started_at: "2026-09-30T10:00:02Z",
				},
			],
			message_buffering: false,
			thought_buffering: false,
		},
	];
}

test("a keyboard-only pass finds a running Session, follows it, opens a diff and takes a turn", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 375, height: 667 });
	const workspace = await opened(request);
	const base = `/operator/organizations/${ORGANIZATION}/workspaces/${workspace.name}`;

	await page.route(
		(url) => url.pathname === `${base}/sessions`,
		async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify(workingSession(workspace)),
			});
		},
	);
	await page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/workspaces`,
		async (route) => {
			if (route.request().method() !== "GET") return route.continue();
			const listed = await route.fetch();
			const body: { name: string }[] = await listed.json();
			const shown = body.find((item) => item.name === workspace.name);
			if (shown) Object.assign(shown, { session: workingSession(workspace)[0], queue: null });
			await route.fulfill({ response: listed, json: body });
		},
	);
	await page.route(
		(url) => url.pathname === `${base}/changes`,
		async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					repositories: [
						{
							repository: "https://github.com/openkestrel/kestrel",
							diff: "diff --git a/keyboard b/keyboard\n+a keyboard diff\n",
							files: [{ path: "keyboard", added: 1, removed: 0 }],
							truncated: false,
						},
					],
				}),
			});
		},
	);

	await page.goto(`/organizations/${ORGANIZATION}`);
	const row = page.locator(`a[href$="/workspaces/${workspace.name}"]`);
	await expect(row).toContainText("Working");
	await tabTo(page, row, "the running Session row");
	await page.keyboard.press("Enter");
	await expect(page).toHaveURL(new RegExp(`/workspaces/${workspace.name}$`));

	const panes = page.getByRole("tablist", { name: "Panes" });
	await tabTo(page, panes.getByRole("tab", { selected: true }), "the selected area tab");
	await page.keyboard.press("ArrowRight");
	await expect(panes.getByRole("tab", { name: "Work", exact: true })).toBeFocused();
	await page.keyboard.press("Enter");
	const work = page.getByRole("region", { name: "Work", exact: true });
	await expect(work).toBeVisible();

	const views = work.getByRole("tablist", { name: "Work views" });
	await tabTo(page, views.getByRole("tab", { selected: true }), "the selected Work view tab");
	await page.keyboard.press("ArrowRight");
	await expect(views.getByRole("tab", { name: "Diff" })).toBeFocused();
	await page.keyboard.press("Enter");
	await expect(work.getByText("a keyboard diff")).toBeVisible();

	await tabTo(page, panes.getByRole("tab", { selected: true }), "the selected area tab");
	await page.keyboard.press("ArrowLeft");
	await expect(panes.getByRole("tab", { name: "Transcript" })).toBeFocused();
	await page.keyboard.press("Enter");
	const composer = page.locator("[data-composer-input]");
	await expect(composer).toBeVisible();
	await tabTo(page, composer, "the composer");
	await page.keyboard.type("a keyboard turn");
	const post = page.getByRole("button", { name: "Post", exact: true });
	await tabTo(page, post, "the Post button");
	await page.keyboard.press("Enter");

	const name = page.getByLabel("Your name");
	await expect(name).toBeVisible();
	await tabTo(page, name, "the name gate");
	await page.keyboard.type("jack");
	await page.keyboard.press("Enter");
	await expect(page.getByLabel("Your name")).toHaveCount(0);

	await tabTo(page, post, "the Post button");
	await page.keyboard.press("Enter");
	await expect(page.getByRole("log").getByText("jack: a keyboard turn")).toBeVisible({
		timeout: 20_000,
	});
	await expect(page.locator("[data-transcript-announcement]")).toHaveText("jack: a keyboard turn");
});

test("Shift+Tab cycles a mode with an announcement, and Escape leaves the composer", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 375, height: 667 });
	const workspace = await opened(request);
	const base = `/operator/organizations/${ORGANIZATION}/workspaces/${workspace.name}`;
	const running = workingSession(workspace)[0];
	await page.route(
		(url) => url.pathname === base,
		async (route) => {
			const record = await route.fetch();
			const body = await record.json();
			await route.fulfill({
				response: record,
				json: {
					...body,
					unfinished_session: {
						id: running?.id,
						name: running?.name,
						state: "working",
						preparing: null,
					},
				},
			});
		},
	);
	await page.route(
		(url) => url.pathname === `${base}/sessions`,
		async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify(workingSession(workspace)),
			});
		},
	);
	await page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/sessions/${running?.id}`,
		async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify(running),
			});
		},
	);

	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	const composer = page.getByLabel("Add to next turn");
	await expect(composer).toBeVisible();
	await composer.focus();

	await page.keyboard.press("Shift+Tab");
	await expect(page.locator("[data-announcement]")).toHaveText(
		"The mode cannot change during a working turn",
	);

	await page.keyboard.press("Escape");
	await expect(page.locator("[data-announcement]")).toHaveText("Left the composer");
	const blurred = await composer.evaluate((element) => element !== document.activeElement);
	expect(blurred).toBe(true);
});
