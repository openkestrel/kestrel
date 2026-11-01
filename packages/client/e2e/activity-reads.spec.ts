import { expect, test } from "@playwright/test";
import {
	caughtUp,
	entry,
	opened,
	prepared,
	Scripted,
	stillOpen,
	summary,
	tool,
	visiting,
	wire,
} from "./activity";

test.beforeAll(async ({ request }) => {
	await prepared(request);
});

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
					tool_kinds: ["read", "edit", "search", "execute", "fetch", "other"],
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
	await Promise.all(
		["read", "edit", "search", "execute", "fetch", "other"].map((kind) =>
			expect(first.getByRole("img", { name: `${kind} tools`, exact: true })).toHaveCount(1),
		),
	);
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
