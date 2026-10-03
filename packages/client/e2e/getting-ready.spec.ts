import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

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
	await request.post(`/operator/organizations/${ORGANIZATION}/profiles`, {
		data: { name: "work", owner: "jack" },
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

function session(index: number, overrides: Record<string, unknown> = {}) {
	return {
		id: `00000000-0000-0000-0000-0000000001${index}`,
		name: `calm-river-abcdefg${index}`,
		workspace: "00000000-0000-0000-0000-0000000000ff",
		state: "unbriefed",
		preparing: "provisioning",
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

function workspaceRecord(
	workspace: { id: string; name: string },
	overrides: Record<string, unknown> = {},
) {
	return {
		id: workspace.id,
		name: workspace.name,
		organization: ORGANIZATION,
		project: "kestrel",
		opened_with: "builder",
		profile: null,
		checkout: {
			repositories: ["https://github.com/openkestrel/kestrel"],
			base: "main",
			branch: `kestrel/${workspace.id}`,
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

type WireEvent = { name: string; id?: string; data: unknown };

function wire(...events: WireEvent[]): string {
	return events
		.map(
			({ name, id, data }) =>
				`${id ? `id: ${id}\n` : ""}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`,
		)
		.join("");
}

class Reads {
	workspace: Record<string, unknown> | undefined;
	sessions: unknown[] | undefined;
	transcript: string | undefined;

	constructor(private readonly workspaceName: string) {}

	async install(page: Page): Promise<void> {
		const base = `/operator/organizations/${ORGANIZATION}/workspaces/${this.workspaceName}`;
		await page.route(
			(url) => url.pathname === base,
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
			(url) => url.pathname === `${base}/sessions`,
			async (route) => {
				if (this.sessions === undefined) {
					await route.continue();
					return;
				}
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify(this.sessions),
				});
			},
		);
		await page.route(
			(url) => url.pathname === `${base}/transcript`,
			async (route) => {
				if (this.transcript === undefined) {
					await route.continue();
					return;
				}
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: this.transcript,
				});
			},
		);
	}
}

test("an open with no Brief lands with the composer, and its first message becomes the Brief", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, undefined);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();

	const message = page.getByLabel("Message");
	await expect(message).toBeVisible();
	await page.getByLabel("Your name").fill("jack");
	await message.fill("the first brief");
	await page.getByRole("button", { name: "Send" }).click();

	await expect(page.getByRole("log").getByText("the first brief")).toBeVisible({ timeout: 20_000 });
	await expect(message).toHaveValue("");
});

test("the preparing line follows the Session from provisioning to harness ready", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, undefined);
	const reads = new Reads(workspace.name);
	reads.sessions = [session(1, { preparing: "provisioning" })];
	await reads.install(page);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	const line = page.locator("[data-preparing]");
	await expect(line).toContainText("Getting ready: provisioning the Instance…");
	await expect(line).toHaveAttribute("aria-live", "polite");

	reads.sessions = [session(1, { preparing: "cloning" })];
	await page.reload();
	await expect(line).toContainText("Getting ready: cloning the checkout…");

	reads.sessions = [session(1, { preparing: "starting_harness" })];
	await page.reload();
	await expect(line).toContainText("Getting ready: starting the harness…");

	reads.sessions = [session(1, { preparing: "harness_ready" })];
	await page.reload();
	await expect(line).toContainText("Ready: the harness is up");
});

test("a message sent before ready shows as held, then becomes the Brief", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, undefined);
	const held = {
		id: 1,
		participant: "jack",
		message: "the first brief",
		posted_at: "2026-09-30T10:00:00Z",
		edited_at: null,
	};
	const reads = new Reads(workspace.name);
	reads.workspace = workspaceRecord(workspace, {
		held_messages: [held],
		unfinished_session: {
			id: session(1).id,
			name: session(1).name,
			state: "unbriefed",
			preparing: "provisioning",
		},
	});
	reads.sessions = [session(1, { preparing: "provisioning" })];
	// The first answer closes at once so the follow reconnects into the Brief.
	reads.transcript = "";
	await reads.install(page);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	await expect(page.locator("[data-held-message]")).toContainText("jack: the first brief");
	await expect(page.locator("[data-preparing]")).toContainText("provisioning the Instance");

	reads.workspace = workspaceRecord(workspace, {
		held_messages: [],
		unfinished_session: {
			id: session(1).id,
			name: session(1).name,
			state: "unbriefed",
			preparing: "harness_ready",
		},
	});
	reads.sessions = [session(1, { preparing: "harness_ready" })];
	reads.transcript = wire(
		{
			name: "entry",
			id: `${workspace.id}:1`,
			data: {
				kind: "shared_state",
				session_id: null,
				seq: 1,
				appended_at: "2026-09-30T10:00:01Z",
				entry: { type: "participant_joined", participant: "jack" },
			},
		},
		{
			name: "entry",
			id: `${workspace.id}:2`,
			data: {
				kind: "shared_state",
				session_id: null,
				seq: 2,
				appended_at: "2026-09-30T10:00:02Z",
				entry: {
					type: "brief",
					source: { kind: "operator", participant: "jack" },
					brief: "the first brief",
				},
			},
		},
		{ name: "end", data: { because: "sealed" } },
	);
	// Only a supervisor can drive this transition live, so the page reloads against the fixtures.
	await page.reload();

	await expect(page.locator("[data-held]")).toHaveCount(0);
	await expect(page.locator("[data-preparing]")).toContainText("Ready: the harness is up");
	await expect(page.getByRole("log").getByText("the first brief")).toBeVisible({ timeout: 20_000 });
});

test("a failure while preparing shows as a failed Session", async ({ page, request }) => {
	const workspace = await opened(request, undefined);
	const reads = new Reads(workspace.name);
	reads.sessions = [
		session(1, {
			state: "ended",
			preparing: null,
			exit: { status: "failed", because: "the spawn failed" },
		}),
	];
	await reads.install(page);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	await expect(page.locator("[data-preparing]")).toContainText(
		"The Session failed: the spawn failed.",
	);
});

test("a sealed Workspace continues into a prefilled New Workspace form", async ({
	page,
	request,
}) => {
	const workspace = await opened(request, undefined);
	const reads = new Reads(workspace.name);
	reads.workspace = workspaceRecord(workspace, {
		state: "sealed",
		sealed_at: "2026-09-30T11:00:00Z",
		profile: "work",
	});
	await reads.install(page);
	await page.goto(`/organizations/${ORGANIZATION}/workspaces/${workspace.name}`);

	const continued = page.getByRole("button", { name: "Continue in a new Workspace" });
	await expect(continued).toBeVisible();
	await continued.focus();
	await continued.press("Enter");

	await expect(page).toHaveURL(new RegExp(`/organizations/${ORGANIZATION}/new$`));
	await expect(page.getByLabel("Project")).toHaveValue("kestrel");
	await expect(page.getByLabel("Agent")).toHaveValue("builder");
	await expect(page.getByLabel("Subscription Profile")).toHaveValue("work");
	await expect(page.getByLabel("Branch")).toHaveValue(`kestrel/${workspace.id}`);
	await expect(page.locator("[data-continues]")).toHaveText(workspace.id);

	const declared = Promise.withResolvers<unknown>();
	await page.route(
		(url) => url.pathname === `/operator/organizations/${ORGANIZATION}/workspaces`,
		async (route) => {
			if (route.request().method() !== "POST") {
				await route.continue();
				return;
			}
			declared.resolve(route.request().postDataJSON());
			await route.fulfill({
				status: 201,
				contentType: "application/json",
				body: JSON.stringify({
					workspace: {
						id: "00000000-0000-0000-0000-0000000000aa",
						name: "continued-otter-abcdefgh",
					},
					session: {},
				}),
			});
		},
	);

	await page.getByLabel("Your name").fill("jack");
	await page.getByRole("button", { name: "Open Workspace" }).click();

	expect(await declared.promise).toMatchObject({
		project: "kestrel",
		agent: "builder",
		profile: "work",
		continues: workspace.id,
		branch: null,
	});
});
