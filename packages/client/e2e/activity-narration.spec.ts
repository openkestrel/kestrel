import { expect, test } from "@playwright/test";
import {
	caughtUp,
	narrated,
	opened,
	prepared,
	Scripted,
	summary,
	visiting,
	wire,
} from "./activity";

test.beforeAll(async ({ request }) => {
	await prepared(request);
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
