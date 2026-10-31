import { type ChildProcess, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type APIRequestContext, type Page, type Route } from "@playwright/test";

const PROJECTS = "/operator/organizations/acme/projects";
const INTEGRATIONS = "/operator/organizations/acme/integrations";
const SETTINGS = "/organizations/acme/settings/repositories";
const FLOW = "safe-flow-reference";
const COMPLETION = `/setup/github?organization=acme&github_app_flow=${FLOW}`;

// Its own install: a Project is never undeclared, and the other suites lean on acme holding none.
let controlPlane: ChildProcess;
let api: APIRequestContext;
let directory: string;

test.beforeAll(async ({ playwright, request }) => {
	directory = await mkdtemp(`${tmpdir()}/kestrel-repositories-`);
	controlPlane = spawn(resolve("../../target/debug/kestrel-control-plane"), ["serve"], {
		env: {
			...process.env,
			KESTREL_DATA_DIR: directory,
			KESTREL_COMPUTE: "local-exec",
			KESTREL_LISTEN: "127.0.0.1:18737",
			KESTREL_OPERATOR_LISTEN: "127.0.0.1:18738",
		},
		stdio: "ignore",
	});
	api = await playwright.request.newContext({ baseURL: "http://127.0.0.1:18738" });
	await expect
		.poll(async () => {
			try {
				return (await api.get("/operator/install")).status();
			} catch {
				return 0;
			}
		})
		.toBe(200);
	await api.post("/operator/organizations", { data: { name: "acme" } });
	await request.post("/operator/organizations", { data: { name: "acme" } });
});

test.afterAll(async () => {
	controlPlane.kill();
	await api.dispose();
	await rm(directory, { recursive: true, force: true });
});

test.beforeEach(async ({ page }) => {
	await page.route(
		(url) => url.pathname.startsWith("/operator/") && !url.pathname.startsWith("/operator/streams"),
		async (route) => {
			const request = route.request();
			const url = new URL(request.url());
			const response = await api.fetch(url.pathname + url.search, {
				method: request.method(),
				data: request.postData() ?? undefined,
				headers: { "content-type": "application/json" },
			});
			await route.fulfill({ response });
		},
	);
});

const github = {
	id: "00000000-0000-0000-0000-00000000000a",
	name: "github",
	kind: "github",
	repository: "acme/repo",
	bot_login: "kestrel-example[bot]",
	carries: ["inbound", "outbound"],
	state: "enabled",
	revision: 3,
	disabled_at: null,
	retired_at: null,
	canceled_posts: [],
	polled_every: "1m",
	webhook_path: null,
	last_event_refusal: null,
	diagnostic: null,
	webhook_secret_rotation: null,
};

function flowOf(phase: string, patch: Record<string, unknown> = {}) {
	return {
		flow: FLOW,
		phase,
		organization: "acme",
		name: "github",
		expires_at: new Date(Date.now() + 45 * 60_000).toISOString(),
		installation_id: phase === "converted" ? null : 23,
		integration: null,
		repository: null,
		guidance: "Create and install the App, then explicitly select a repository.",
		...patch,
	};
}

const PAGES = [
	{
		repositories: [
			{ id: 1, repository: "acme/site", default_branch: "main", private: false },
			{ id: 2, repository: "acme/tools", default_branch: "main", private: true },
		],
		page: 1,
		total: 3,
		next_page: 2,
	},
	{
		repositories: [{ id: 3, repository: "acme/repo", default_branch: "trunk", private: true }],
		page: 2,
		total: 3,
		next_page: null,
	},
];

async function stubGithub(page: Page, held: { phase: string; finished: string[] }) {
	await page.route(`**/operator/github-app/flows/${FLOW}`, (route) =>
		route.fulfill({
			json:
				held.finished.length > 0
					? flowOf("completed", { integration: "github", repository: held.finished[0] })
					: flowOf(held.phase),
		}),
	);
	await page.route(`**/operator/github-app/flows/${FLOW}/repositories?*`, (route) => {
		const asked = Number(new URL(route.request().url()).searchParams.get("page"));
		return route.fulfill({ json: PAGES[asked - 1] });
	});
	await page.route(`**/operator/github-app/flows/${FLOW}/finish`, (route) => {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the Client's finish request.
		const { repository } = route.request().postDataJSON() as { repository: string };
		held.finished.push(repository);
		return route.fulfill({
			json: flowOf("completed", { integration: "github", repository }),
		});
	});
	await page.route(
		(url) => url.pathname === INTEGRATIONS,
		(route) => route.fulfill({ json: [github] }),
	);
	await page.route("**/operator/repository-resolutions", (route) => {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the Client's resolution request.
		const asked = route.request().postDataJSON() as {
			repositories: string[];
			integrations?: { repository: string; integration: string }[];
		};
		return route.fulfill({
			json: {
				repositories: asked.repositories.map((given) => ({
					given,
					address: `https://github.com/${given}.git`,
					checkout_directory: given.split("/").at(-1),
					integration:
						asked.integrations?.find(({ repository }) => repository === given)?.integration ?? null,
				})),
				warnings: [],
			},
		});
	});
}

function projectForm(page: Page) {
	return page.locator("[data-project-form]");
}

function review(page: Page) {
	return page.locator("[data-project-review]");
}

test("a repository is chosen only after the App is installed, across pages and a reload of the completion route", async ({
	page,
}) => {
	const held = { phase: "converted", finished: [] as string[] };
	await stubGithub(page, held);
	const declared: unknown[] = [];
	await page.route(
		(url) => url.pathname === PROJECTS,
		(route: Route) => {
			if (route.request().method() !== "POST") return route.fulfill({ json: [] });
			declared.push(route.request().postDataJSON());
			return route.fulfill({
				status: 201,
				json: {
					id: "00000000-0000-0000-0000-000000000002",
					name: "repo",
					repositories: ["https://github.com/acme/repo.git"],
					integrations: [
						{
							repository: "https://github.com/acme/repo.git",
							integration: "github",
							integration_id: github.id,
						},
					],
					branch: "trunk",
				},
			});
		},
	);

	await page.goto(COMPLETION);
	await expect(page.locator('[data-flow-stage="installing"]')).toContainText(
		"a repository can be chosen only once it is installed",
	);
	await expect(page.locator("[data-flow-expiry]")).toContainText(/Expires in 4[56] minutes/);
	await expect(page.locator("[data-flow-expiry]")).toContainText(
		"Nothing here deletes or uninstalls the GitHub App",
	);
	await expect(page.locator("[data-repository-picker]")).toHaveCount(0);

	held.phase = "select_repository";
	await page.getByRole("button", { name: "Check the installation again" }).click();
	const picker = page.locator("[data-repository-picker]");
	await expect(picker.locator("[data-repository]")).toHaveText([/acme\/site/, /acme\/tools/]);
	await expect(picker.locator("[data-repositories-page]")).toHaveText("Page 1 · 3 repositories");
	await expect(picker.getByRole("button", { name: "Previous page" })).toBeDisabled();

	const { violations } = await new AxeBuilder({ page }).analyze();
	expect(violations.filter(({ impact }) => impact === "serious" || impact === "critical")).toEqual(
		[],
	);

	await picker.getByRole("button", { name: "Next page" }).click();
	await expect(picker.locator("[data-repository]")).toHaveText([/acme\/repo.*trunk.*private/]);
	await expect(picker.getByRole("button", { name: "Next page" })).toBeDisabled();

	await page.reload();
	await expect(picker.locator("[data-repository]")).toHaveText([/acme\/site/, /acme\/tools/]);
	await picker.getByRole("button", { name: "Next page" }).click();
	await picker.getByLabel(/acme\/repo/).check();
	await picker.getByRole("button", { name: "Connect acme/repo" }).click();
	expect(held.finished).toEqual(["acme/repo"]);

	await expect(page.locator("[data-flow-expiry]")).toHaveText(
		"Connected the repository acme/repo as the Integration github.",
	);
	await expect(projectForm(page).getByLabel("Project name")).toHaveValue("repo");
	await expect(projectForm(page).getByLabel("Repository", { exact: true })).toHaveValue(
		"acme/repo",
	);
	await expect(projectForm(page).getByLabel("Reach GitHub as")).toHaveValue("github");
	await expect(projectForm(page).getByLabel("Checkout base branch")).toHaveValue("trunk");
	await expect(projectForm(page).locator("#project-branch-hint")).toContainText(
		"Each Workspace works on a branch of its own",
	);

	await projectForm(page).getByRole("button", { name: "Review" }).click();
	await expect(review(page).locator("[data-address]")).toHaveText(
		"https://github.com/acme/repo.git",
	);
	await expect(review(page).locator("[data-authority]")).toHaveText(
		"Reaches GitHub as the Integration github.",
	);
	await expect(review(page).locator("[data-base-branch]")).toHaveText("trunk");
	await expect(review(page).locator("[data-save-claim]")).toContainText(
		"not that a checkout will succeed",
	);
	await review(page).getByRole("button", { name: "Save Project" }).click();
	await expect(page.locator("[data-flow-project]")).toHaveText(
		"Saved the Project repo, which reaches GitHub as github.",
	);
	expect(declared).toEqual([
		{
			name: "repo",
			repositories: ["acme/repo"],
			integrations: [{ repository: "acme/repo", integration: "github" }],
			branch: "trunk",
		},
	]);

	await page.reload();
	await expect(page.locator("[data-flow-expiry]")).toHaveText(
		"Connected the repository acme/repo as the Integration github.",
	);
	await expect(projectForm(page).getByLabel("Repository", { exact: true })).toHaveValue(
		"acme/repo",
	);
	await expect(projectForm(page).getByLabel("Reach GitHub as")).toHaveValue("github");
	await expect(projectForm(page).getByLabel("Checkout base branch")).toHaveValue("");
});

test("connecting starts an App flow from the Client's own origin and returns to the completion route", async ({
	page,
}) => {
	await stubGithub(page, { phase: "select_repository", finished: [] });
	const started: unknown[] = [];
	await page.route("**/operator/organizations/acme/github-app", (route) => {
		started.push(route.request().postDataJSON());
		return route.fulfill({ status: 201, json: { url: COMPLETION, flow: FLOW } });
	});
	await page.goto(SETTINGS);
	await page.getByRole("button", { name: "Connect GitHub" }).click();
	await page.getByLabel("GitHub organization").fill("acme-inc");
	await page.getByRole("button", { name: "Create the App on GitHub" }).click();
	await expect(page).toHaveURL(
		new RegExp(`/setup/github\\?organization=acme&github_app_flow=${FLOW}$`),
	);
	await expect(page.locator("[data-repository-picker]")).toBeVisible();
	expect(started).toEqual([
		{ name: "github", callback_base: new URL(page.url()).origin, app_organization: "acme-inc" },
	]);
});

test("an expired or cancelled setup offers a new one and never claims to remove the App", async ({
	page,
}) => {
	const held = { phase: "select_repository", finished: [] as string[] };
	await stubGithub(page, held);
	await page.route(`**/operator/github-app/flows/${FLOW}`, (route) => {
		if (route.request().method() === "DELETE") {
			held.phase = "cancelled";
			return route.fulfill({ json: flowOf("cancelled") });
		}
		return route.fallback();
	});
	await page.goto(COMPLETION);
	await page.getByRole("button", { name: "Cancel this GitHub setup" }).click();
	const ended = page.locator('[data-flow-stage="ended"]');
	await expect(ended).toContainText("This GitHub setup was cancelled.");
	await expect(ended.locator("[data-connect-github]")).toBeVisible();
	await expect(page.locator("[data-flow-expiry]")).not.toContainText("Expires in");

	held.phase = "expired";
	await page.route(`**/operator/github-app/flows/${FLOW}`, (route) =>
		route.fulfill({
			json: flowOf("expired", { expires_at: new Date(Date.now() - 60_000).toISOString() }),
		}),
	);
	await page.reload();
	await expect(ended).toContainText("expired before a repository was chosen");
	await expect(page.locator("[data-flow-expiry]")).toContainText("Expired");
	await expect(page.locator("[data-flow-expiry]")).toContainText(
		"Nothing here deletes or uninstalls the GitHub App",
	);
});

test("a failed repository page says so and is asked again, never shown as empty", async ({
	page,
}) => {
	await stubGithub(page, { phase: "select_repository", finished: [] });
	const reads = { failing: true };
	await page.route(`**/operator/github-app/flows/${FLOW}/repositories?*`, (route) =>
		reads.failing
			? route.fulfill({
					status: 503,
					json: {
						kind: "unavailable",
						message: "GitHub did not answer",
						field: null,
						context: {
							service: "github",
							resource: null,
							operation: "list_github_app_repositories",
							retry_after_seconds: null,
						},
						next_steps: [
							{
								action: "retry_read",
								operation: "list_github_app_repositories",
								resource: null,
								retry_after_seconds: null,
							},
						],
					},
				})
			: route.fallback(),
	);
	await page.goto(COMPLETION);
	const failed = page.locator('[data-repositories-read="failed"]');
	await expect(failed).toContainText("GitHub did not answer", { timeout: 15_000 });
	await expect(page.locator('[data-repositories-read="empty"]')).toHaveCount(0);
	reads.failing = false;
	await failed.getByRole("button", { name: "Read again" }).click();
	await expect(page.locator("[data-repository]")).toHaveCount(2);
});

test("a refused save keeps the saved Project and every input, then saves on the next try", async ({
	page,
}) => {
	await api.post(PROJECTS, {
		data: { name: "kestrel", repositories: ["acme/kestrel"], branch: "main" },
	});
	const writes = { refusing: true };
	await page.route(
		(url) => url.pathname === PROJECTS,
		(route) =>
			route.request().method() === "POST" && writes.refusing
				? route.fulfill({
						status: 422,
						json: {
							kind: "invalid_field",
							message: "The branch develop does not exist",
							field: "branch",
							context: { operation: "declare_project", resource: "project", organization: "acme" },
							next_steps: [
								{
									action: "correct_field",
									field: "branch",
									constraint: "exists",
									allowed_values: null,
								},
							],
						},
					})
				: route.fallback(),
	);
	await page.goto(SETTINGS);
	const listed = page.locator('[data-project="kestrel"]');
	await expect(listed).toContainText("checkout base branch main");
	await expect(listed).toContainText("https://github.com/acme/kestrel.git · no App authority");

	await page.getByRole("button", { name: "Edit kestrel" }).click();
	await expect(projectForm(page).getByLabel("Project name")).toHaveAttribute("readonly");
	await projectForm(page).getByLabel("Checkout base branch").fill("develop");
	await projectForm(page).getByRole("button", { name: "Add a repository" }).click();
	await projectForm(page).getByLabel("Repository 2").fill("acme/docs");
	await projectForm(page).getByRole("button", { name: "Review" }).click();
	await expect(review(page).locator("[data-address]")).toHaveText([
		"https://github.com/acme/kestrel.git",
		"https://github.com/acme/docs.git",
	]);
	await expect(review(page).locator("[data-save-claim]")).toHaveText(
		"Saving accepts the address's form only; it does not reach the repository or prove a checkout.",
	);
	await review(page).getByRole("button", { name: "Save Project" }).click();

	await expect(projectForm(page).getByRole("alert")).toHaveText(
		"The branch develop does not exist",
	);
	await expect(projectForm(page).getByLabel("Checkout base branch")).toBeFocused();
	await expect(projectForm(page).getByLabel("Checkout base branch")).toHaveValue("develop");
	await expect(projectForm(page).getByLabel("Repository 2")).toHaveValue("acme/docs");
	await expect(listed).toContainText("checkout base branch main");
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's Project list.
	const before = (await (await api.get(PROJECTS)).json()) as { branch: string }[];
	expect(before).toMatchObject([{ branch: "main" }]);

	writes.refusing = false;
	await projectForm(page).getByRole("button", { name: "Review" }).click();
	await review(page).getByRole("button", { name: "Save Project" }).click();
	await expect(page.getByRole("status")).toHaveText("Saved the Project kestrel.");
	await expect(listed).toContainText("checkout base branch develop");
	await expect(listed).toContainText("https://github.com/acme/docs.git");
});

test("a failed Projects read keeps what was entered and reads again; none declared offers a declaration", async ({
	page,
}) => {
	const reads = { failing: true, empty: true };
	await page.route(
		(url) => url.pathname === PROJECTS,
		(route) => {
			if (route.request().method() !== "GET") return route.fallback();
			if (reads.failing)
				return route.fulfill({
					status: 503,
					json: {
						kind: "unavailable",
						message: "The store is busy",
						field: null,
						context: {
							service: "store",
							resource: null,
							operation: "list_projects",
							retry_after_seconds: null,
						},
						next_steps: [
							{
								action: "retry_read",
								operation: "list_projects",
								resource: null,
								retry_after_seconds: null,
							},
						],
					},
				});
			return reads.empty ? route.fulfill({ json: [] }) : route.fallback();
		},
	);
	await page.goto(SETTINGS);
	const failed = page.locator('[data-projects-read="failed"]');
	await expect(failed).toContainText("The store is busy", { timeout: 15_000 });
	await expect(page.locator('[data-projects-read="empty"]')).toHaveCount(0);

	await page.getByRole("button", { name: "Declare a Project" }).click();
	await projectForm(page).getByLabel("Project name").fill("kept");
	await projectForm(page).getByLabel("Repository", { exact: true }).fill("acme/kept");
	await projectForm(page).getByLabel("Checkout base branch").fill("main");

	reads.failing = false;
	await failed.getByRole("button", { name: "Read again" }).click();
	await expect(page.locator('[data-projects-read="empty"]')).toContainText(
		"No Project is declared yet.",
	);
	await expect(projectForm(page).getByLabel("Project name")).toHaveValue("kept");
	await expect(projectForm(page).getByLabel("Repository", { exact: true })).toHaveValue(
		"acme/kept",
	);
	await expect(page.locator('[data-integrations-read="empty"]')).toContainText(
		"No GitHub Integration is connected.",
	);
});

test("setup takes a public repository, shows its resolved address and claims no push or checkout", async ({
	page,
	request,
}) => {
	// acme holds Projects by now, and setup asks for a repository only where none is saved.
	await api.post("/operator/organizations", { data: { name: "beta" } });
	await request.post("/operator/organizations", { data: { name: "beta" } });
	await api.put("/operator/operator", { data: { name: "river" } });
	await page.goto("/setup?organization=beta");
	await expect(page.getByText("Which repository should the agent work on?")).toBeVisible();
	await page.getByRole("button", { name: "Use a public repository" }).click();
	await projectForm(page).getByLabel("Project name").fill("public");
	await projectForm(page).getByLabel("Repository", { exact: true }).fill("acme/public");
	await expect(projectForm(page).getByLabel("Reach GitHub as")).toHaveCount(0);
	await projectForm(page).getByLabel("Checkout base branch").fill("main");
	await projectForm(page).getByRole("button", { name: "Review" }).click();
	await expect(review(page).locator("[data-address]")).toHaveText(
		"https://github.com/acme/public.git",
	);
	await expect(review(page).locator("[data-authority]")).toContainText(
		"kestrel lends it nothing to push with",
	);
	await review(page).getByRole("button", { name: "Save Project" }).click();
	await expect(page.getByLabel("Saved prerequisites")).toContainText("public");
	await page.getByRole("button", { name: "Back", exact: true }).click();
	await expect(page.locator("[data-saved-projects]")).toContainText("public");
});
