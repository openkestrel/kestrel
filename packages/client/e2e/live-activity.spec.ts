import { expect, test } from "@playwright/test";
import {
	entry,
	opened,
	prepared,
	Scripted,
	stillOpen,
	summary,
	visiting,
	wire,
	working,
} from "./activity";

test.beforeAll(async ({ request }) => {
	await prepared(request);
});

test("the live Activity says the agent is thinking while a thought is buffering", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: (attempt) => (attempt === 1 ? wire(working([], { thought: true })) : stillOpen()),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await expect(log.getByRole("button", { name: "Activity, Working", exact: true })).toHaveAttribute(
		"aria-expanded",
		"true",
	);
	await expect(log.getByRole("listitem")).toHaveText(["Thinking…"]);
});

test("an Activity is replaced as it grows, and a reconnect adds no duplicate entries", async ({
	page,
	request,
}) => {
	const workspace = await opened(request);
	const scripted = new Scripted({
		follow: (attempt) =>
			attempt === 1
				? wire(
						entry(workspace, 1, { type: "said", participant: "jack", message: "hello" }),
						summary(workspace, 2, 3, {
							counts: { tool_calls: 1, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
							closed: false,
							latest: { kind: "detail", title: "tool 2", status: "in_progress" },
						}),
					)
				: wire(
						summary(workspace, 2, 5, {
							counts: { tool_calls: 2, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
							latest: { kind: "detail", title: "tool 5", status: "completed" },
						}),
						entry(workspace, 6, { type: "said", participant: "jack", message: "done" }),
						{ name: "end", data: { because: "sealed" } },
					),
	});
	await scripted.install(page);
	await visiting(page, workspace);

	const log = page.getByRole("log");
	await expect(
		log.getByRole("article", { name: "jack" }).filter({ hasText: "done" }),
	).toBeVisible();
	await expect(log.getByRole("button", { name: /^Activity/ })).toHaveCount(1);
	await expect(
		log.getByRole("button", { name: "Activity, 2 tool calls", exact: true }),
	).toHaveAttribute("aria-expanded", "false");
	await expect(log.getByRole("article", { name: "jack" }).filter({ hasText: "hello" })).toHaveCount(
		1,
	);
	await expect(log.getByRole("article", { name: "jack" }).filter({ hasText: "done" })).toHaveCount(
		1,
	);

	const resumed = scripted.streamed.subscriptions.filter(
		(subscribed) => subscribed.kind === "transcript",
	);
	expect(resumed).toHaveLength(2);
	expect(resumed[0]?.after).toBeUndefined();
	expect(resumed[1]?.after).toBe(`${workspace.id}:3`);
	expect(resumed[1]?.token).not.toBe(resumed[0]?.token);
});
