import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createConnection, createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, test } from "@playwright/test";

const OPERATOR = 17718;
const CLIENT = 17729;
const CONTROL_PLANE_UNTIL_STARTED = 17728;

let client: ChildProcess;
let started: Server | undefined;

test.beforeAll(async () => {
	client = spawn(
		"caddy",
		["run", "--adapter", "caddyfile", "--config", "../../images/kestrel-client/Caddyfile"],
		{
			cwd: resolve(import.meta.dirname, ".."),
			stdio: "ignore",
			env: {
				...process.env,
				XDG_DATA_HOME: mkdtempSync(join(tmpdir(), "kestrel-client-e2e-outage-")),
				KESTREL_CLIENT_LISTEN_PORT: String(CLIENT),
				KESTREL_CLIENT_ROOT: resolve(import.meta.dirname, "../dist/client"),
				KESTREL_CONTROL_PLANE: `http://127.0.0.1:${CONTROL_PLANE_UNTIL_STARTED}`,
				KESTREL_COMPOSE: "true",
			},
		},
	);
	await expect
		.poll(() =>
			fetch(`http://127.0.0.1:${CLIENT}/`).then(
				(answer) => answer.status,
				() => 0,
			),
		)
		.toBe(200);
});

test.afterAll(() => {
	client.kill();
	started?.close();
});

test("a Client whose control plane isn't running says so, and carries on once it is", async ({
	page,
	request,
}) => {
	await request.post("/operator/organizations", { data: { name: "acme" } });

	await page.goto(`http://127.0.0.1:${CLIENT}/organizations/acme`);

	const outage = page.locator("[data-outage]");
	await expect(outage.getByRole("heading", { name: "kestrel isn't running" })).toBeVisible();
	await expect(outage).toContainText("docker compose ps");
	await expect(outage).toContainText("docker compose logs kestrel");
	await page.evaluate(() => {
		document.documentElement.dataset.loaded = "once";
	});

	started = createServer((socket) => {
		const upstream = createConnection(OPERATOR, "127.0.0.1");
		socket.pipe(upstream).pipe(socket);
		upstream.on("error", () => socket.destroy());
		socket.on("error", () => upstream.destroy());
	}).listen(CONTROL_PLANE_UNTIL_STARTED, "127.0.0.1");

	await expect(outage).toBeHidden({ timeout: 20_000 });
	await expect(page.getByRole("heading", { name: "Workspaces" })).toBeVisible();
	await expect(page.locator("html")).toHaveAttribute("data-loaded", "once");
});

test("a control plane that answers with a refusal is running, not stopped", async ({ page }) => {
	await page.route("**/operator/organizations", (route) =>
		route.fulfill({
			status: 503,
			contentType: "application/json",
			body: JSON.stringify({
				kind: "unavailable",
				message: "the control plane could not answer",
				field: null,
				context: {
					service: "control_plane",
					resource: null,
					operation: "list_organizations",
					retry_after_seconds: null,
				},
				next_steps: [],
			}),
		}),
	);

	await page.goto("/");

	await expect(page.getByRole("alert")).toContainText("the control plane could not answer");
	await expect(page.locator("[data-outage]")).toHaveCount(0);
});
