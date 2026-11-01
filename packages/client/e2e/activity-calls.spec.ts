import { expect, test } from "@playwright/test";
import {
	caughtUp,
	COMPLETION,
	opened,
	prepared,
	Scripted,
	stillOpen,
	summary,
	tool,
	visiting,
	wire,
	working,
} from "./activity";

test.beforeAll(async ({ request }) => {
	await prepared(request);
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
	const activity = log.getByRole("button", { name: "Activity, Working", exact: true });
	await activity.click();
	await expect(activity).toHaveAttribute("aria-expanded", "false");
	await expect(activity.getByRole("img", { name: "execute tools", exact: true })).toBeVisible();
	expect(scripted.ranges).toEqual([]);
	await activity.click();
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

test("a folded live Activity combines recorded and running tool kinds once", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: (attempt) =>
			attempt === 1
				? wire(
						summary(workspace, 1, 1, { tool_kinds: ["read"], closed: false }),
						working(
							["read", "execute", "execute"].map((kind, index) => ({
								call_id: `running-${index}`,
								title: "work",
								tool_kind: kind,
								status: "in_progress",
								started_at: "2026-09-30T10:00:00Z",
							})),
						),
					)
				: stillOpen(),
	});
	await scripted.install(page);
	await visiting(page, workspace);
	const activity = page
		.getByRole("log")
		.getByRole("button", { name: "Activity, Working", exact: true });
	await activity.click();
	await expect(activity).toHaveAttribute("aria-expanded", "false");
	await expect(activity.getByRole("img", { name: "read tools", exact: true })).toHaveCount(1);
	await expect(activity.getByRole("img", { name: "execute tools", exact: true })).toHaveCount(1);
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
