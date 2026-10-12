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

async function opened(
	request: APIRequestContext,
	briefed = true,
): Promise<{ id: string; name: string }> {
	const response = await request.post(`/operator/organizations/${ORGANIZATION}/workspaces`, {
		data: { project: "kestrel", agent: "builder", brief: briefed ? "an opening brief" : undefined },
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

// A Workspace whose Session is working, so the Transcript holds the live composer.
async function working(page: Page, workspace: { id: string; name: string }): Promise<void> {
	const running = workingSession(workspace)[0];
	const base = `/operator/organizations/${ORGANIZATION}/workspaces/${workspace.name}`;
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
						id: running.id,
						name: running.name,
						state: "working",
						preparing: null,
					},
				},
			});
		},
	);
	await page.route(
		(url) => url.pathname === `${base}/sessions`,
		(route) =>
			route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify(workingSession(workspace)),
			}),
	);
	await page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/sessions/${running.id}`,
		(route) =>
			route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify(running),
			}),
	);
	await page.route(
		(url) => url.pathname === `${base}/changes`,
		(route) =>
			route.fulfill({
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
			}),
	);
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
	// The new route is only the old one's tablist away once it has actually rendered.
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();

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
	await expect(views.getByRole("tab", { name: "Changes" })).toBeFocused();
	await page.keyboard.press("Enter");
	await expect(work.getByRole("button", { name: /keyboard/ })).toBeVisible();

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
	await expect(
		page
			.getByRole("log")
			.getByRole("article", { name: "jack" })
			.filter({ hasText: "a keyboard turn" }),
	).toBeVisible({
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
						id: running.id,
						name: running.name,
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
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/sessions/${running.id}`,
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

test("Command-B folds the Workspaces pane and Command-Option-B the Work pane, independently", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request);
	await working(page, workspace);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	const workspaces = page.locator('[data-sidebar="heading"][data-side="workspaces"]');
	const workspacesPane = page.getByRole("region", { name: "Workspaces", exact: true });
	const workPane = page.getByRole("region", { name: "Work", exact: true });

	await expect(workspaces).toBeVisible();
	await expect(workspaces).toHaveAccessibleName("Toggle the Workspaces pane");
	await expect(workspaces).toHaveAttribute("aria-expanded", "true");
	await expect(workspacesPane).toBeVisible();

	// Only the platform's own modifier folds: Control on an Apple keyboard is another shortcut's.
	await page.keyboard.press(`${process.platform === "darwin" ? "Control" : "Meta"}+b`);
	await expect(workspaces).toHaveAttribute("aria-expanded", "true");
	await expect(page.locator("[data-workbench-announcement]")).toHaveText("");

	await page.keyboard.press("ControlOrMeta+b");
	const workspacesRail = page.locator('[data-sidebar="rail"][data-side="workspaces"]');
	await expect(workspacesRail).toBeVisible();
	await expect(workspacesRail).toHaveAccessibleName("Toggle the Workspaces pane");
	await expect(workspacesRail).toHaveAttribute("aria-expanded", "false");
	await expect(workspacesPane).toBeHidden();
	await expect(workPane).toBeVisible();
	await expect(page.locator("[data-workbench-announcement]")).toHaveText(
		"Workspaces pane collapsed",
	);

	await page.keyboard.press("ControlOrMeta+Alt+b");
	const workRail = page.locator('[data-sidebar="rail"][data-side="work"]');
	await expect(workRail).toBeVisible();
	await expect(workRail).toHaveAttribute("aria-expanded", "false");
	await expect(workPane).toBeHidden();
	await expect(workspacesPane).toBeHidden();
	await expect(page.locator("[data-workbench-announcement]")).toHaveText("Work pane collapsed");

	await workspacesRail.click();
	await expect(workspacesPane).toBeVisible();
	await expect(workPane).toBeHidden();
	await expect(workspaces).toHaveAttribute("aria-expanded", "true");
});

test("folding a pane a reader is inside leaves their focus on its toggle", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request);
	await working(page, workspace);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	const work = page.getByRole("region", { name: "Work", exact: true });
	const changes = work.getByRole("tab", { name: "Changes" });
	await changes.click();
	await expect(changes).toHaveAttribute("aria-selected", "true");
	await changes.focus();
	await expect(changes).toBeFocused();

	await page.keyboard.press("ControlOrMeta+Alt+b");
	const rail = page.locator('[data-sidebar="rail"][data-side="work"]');
	await expect(rail).toBeVisible();
	await expect(rail).toBeFocused();
	await expect(work).toBeHidden();
});

test("the skip link is the first stop and lands on the composer", async ({ page, request }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request);
	await working(page, workspace);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
	await expect(page.locator("[data-composer-input]")).toBeVisible();

	const skip = page.getByRole("link", { name: "Skip to the composer", exact: true });
	await page.keyboard.press("Tab");
	await expect(skip).toBeFocused();
	// Focusing reveals it, which a link left hidden at rest cannot do.
	expect(await skip.evaluate((element) => element.getBoundingClientRect().width)).toBeGreaterThan(
		20,
	);

	await page.keyboard.press("Enter");
	await expect(page.locator("[data-composer-input]")).toBeFocused();
});

test("the skip link lands on the Brief composer while a Workspace has no Brief", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request, false);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	const message = page.getByRole("textbox", { name: "Message", exact: true });
	await expect(message).toBeVisible();

	await page.keyboard.press("Tab");
	await expect(page.getByRole("link", { name: "Skip to the composer", exact: true })).toBeFocused();
	await page.keyboard.press("Enter");
	await expect(message).toBeFocused();
});

test("a sealed Workspace has no composer, so it offers no link to one", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request);
	await page.route(
		(url) =>
			url.pathname === `/operator/organizations/${ORGANIZATION}/workspaces/${workspace.name}`,
		async (route) => {
			const record = await route.fetch();
			await route.fulfill({
				response: record,
				json: { ...(await record.json()), state: "sealed", sealed_at: "2026-09-30T11:00:00Z" },
			});
		},
	);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
	await expect(page.getByRole("region", { name: "Work", exact: true })).toBeVisible();

	await expect(page.getByRole("textbox", { name: /^(Post|Message)$/ })).toHaveCount(0);
	await expect(page.getByRole("link", { name: /composer/ })).toHaveCount(0);
});

test("Changes reaches the composer in one Tab", async ({ page, request }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	const workspace = await opened(request);
	await working(page, workspace);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	const work = page.getByRole("region", { name: "Work", exact: true });
	const changes = work.getByRole("tab", { name: "Changes" });
	await changes.click();
	await expect(work.getByRole("button", { name: /keyboard/ })).toBeVisible();
	await expect(page.locator("[data-composer-input]")).toBeVisible();
	await changes.focus();

	await page.keyboard.press("Tab");
	const jump = work.getByRole("link", { name: "Go to the composer", exact: true });
	await expect(jump).toBeFocused();

	await page.keyboard.press("Enter");
	await expect(page.locator("[data-composer-input]")).toBeFocused();
});

test("at 375 px the sidebars are tabs, with no sideways scroll, and the skip link still works", async ({
	page,
	request,
}) => {
	await page.setViewportSize({ width: 375, height: 667 });
	const workspace = await opened(request);
	await working(page, workspace);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
	await expect(page.locator("[data-composer-input]")).toBeVisible();

	await expect(page.getByRole("tablist", { name: "Panes" })).toBeVisible();
	await expect(page.locator('[data-sidebar="heading"][data-side="workspaces"]')).toBeHidden();
	await expect(page.locator('[data-sidebar="heading"][data-side="work"]')).toBeHidden();
	await expect(page.locator('[data-sidebar="rail"]')).toHaveCount(0);
	const width = await page.evaluate(() => ({
		document: document.documentElement.scrollWidth,
		viewport: window.innerWidth,
	}));
	expect(width.document, "the page scrolls horizontally").toBeLessThanOrEqual(width.viewport);

	await page.keyboard.press("ControlOrMeta+b");
	await page.keyboard.press("ControlOrMeta+Alt+b");
	await expect(page.locator("[data-workbench-announcement]")).toHaveText("");

	await page.keyboard.press("Tab");
	const skip = page.getByRole("link", { name: "Skip to the composer", exact: true });
	await expect(skip).toBeFocused();
	await page.keyboard.press("Enter");
	await expect(page.locator("[data-composer-input]")).toBeFocused();
	await expect(page.getByRole("region", { name: "Transcript", exact: true })).toBeVisible();
});
