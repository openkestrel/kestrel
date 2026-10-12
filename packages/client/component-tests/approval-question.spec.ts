import { AxeBuilder } from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("an Approval names the action, Policy and answerers and can be allowed from the keyboard", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html");
	const approval = page.getByRole("region", { name: "Approval: Push the branch" });
	await expect(approval).toContainText("git push --force-with-lease origin jack/flaky-usage");
	await expect(approval).toContainText("Policy protected-push");
	await expect(approval).toContainText("jack or sam may answer");
	await expect(approval.getByRole("status")).toHaveText(
		"Pending · Deadline: 12 October 2026, 14:30 UTC",
	);
	await page.keyboard.press("Tab");
	await expect(approval.getByRole("button", { name: "Reject", exact: true })).toBeFocused();
	await page.keyboard.press("Tab");
	await expect(approval.getByRole("button", { name: "Allow", exact: true })).toBeFocused();
	await page.keyboard.press("Enter");
	await expect(approval.getByRole("status")).toHaveText("Allowed by jack.");
	await expect(approval.getByRole("status")).toBeFocused();
	await expect(approval.getByRole("button")).toHaveCount(0);
});

test("Reject resolves an Approval, while a refused answer reports the refusal", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html");
	const approval = page.getByRole("region", { name: "Approval: Push the branch" });
	await approval.getByRole("button", { name: "Reject", exact: true }).press("Enter");
	await expect(approval.getByRole("status")).toHaveText("Rejected by jack.");
	await expect(approval.getByRole("status")).toBeFocused();
	await page.goto("/component-tests/approval-question.html?refuse-answer");
	await approval.getByRole("button", { name: "Allow", exact: true }).press("Enter");
	await expect(approval.getByRole("alert")).toHaveText(
		"Answer refused: Only jack or sam may answer this Approval.",
	);
	await expect(approval.getByRole("alert")).toBeFocused();
	await expect(approval.getByRole("button")).toHaveCount(0);
});

test("Question expiry leaves judgment to the agent, while Approval expiry denies the action", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html?approval=expired&question=expired");
	const question = page.getByRole("region", { name: "Question", exact: true });
	await expect(question.getByRole("status")).toHaveText(
		"Question expired without an answer. The agent proceeds on its own judgment.",
	);
	await expect(question.getByRole("button")).toHaveCount(0);
	await expect(question.getByRole("textbox")).toHaveCount(0);
	await expect(
		page.getByRole("region", { name: "Approval: Push the branch" }).getByRole("status"),
	).toHaveText("Approval expired. The action was denied.");
	await expect(page.getByRole("button")).toHaveCount(0);
});

test("multipart Questions explain an unanswered part before continuing", async ({ page }) => {
	await page.goto("/component-tests/approval-question.html?multipart");
	const question = page.getByRole("region", { name: "Question", exact: true });
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect(question.getByRole("alert")).toHaveText("Choose an answer to continue.");
	await expect(question.getByRole("progressbar")).toHaveAttribute(
		"aria-valuetext",
		"Question 1 of 2",
	);
});

for (const colorScheme of ["light", "dark"] as const) {
	for (const width of [1280, 375]) {
		test(`fixture states remain accessible at ${width}px in ${colorScheme}`, async ({ page }) => {
			await page.setViewportSize({ width, height: 800 });
			await page.emulateMedia({ colorScheme });
			for (const [query, approvalText, questionText] of [
				["", "Pending", "On expiry, the agent proceeds on its own judgment."],
				[
					"?approval=allowed&question=responded",
					"Allowed by sam.",
					"Answered by sam: Open an issue",
				],
				[
					"?approval=rejected&question=expired",
					"Rejected by sam.",
					"Question expired without an answer.",
				],
				[
					"?approval=refused&multipart",
					"Answer refused",
					"On expiry, the agent proceeds on its own judgment.",
				],
				[
					"?approval=expired&question=responded&multipart",
					"Approval expired",
					"Answered by sam: Open an issue; CI",
				],
			]) {
				// oxlint-disable-next-line no-await-in-loop -- each fixture is a separate rendered state.
				await page.goto(`/component-tests/approval-question.html${query}`);
				// oxlint-disable-next-line no-await-in-loop -- assert the state before inspecting accessibility.
				await expect(page.getByRole("region", { name: "Approval: Push the branch" })).toContainText(
					approvalText,
				);
				// oxlint-disable-next-line no-await-in-loop -- assert the same fixture's Question state.
				await expect(page.getByRole("region", { name: "Question", exact: true })).toContainText(
					questionText,
				);
				// oxlint-disable-next-line no-await-in-loop -- layout is inspected before changing this fixture.
				const fits = await page.evaluate(
					() => document.documentElement.scrollWidth <= window.innerWidth,
				);
				expect(fits).toBe(true);
				// oxlint-disable-next-line no-await-in-loop -- axe examines the current fixture before navigation.
				const accessibility = await new AxeBuilder({ page }).analyze();
				expect(accessibility.violations).toEqual([]);
			}
		});
	}
}

test("a multipart Question keeps its answers while navigating and submits them together", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html?multipart");
	const question = page.getByRole("region", { name: "Question", exact: true });
	await expect(
		question.getByRole("progressbar", { name: "Questionnaire progress" }),
	).toHaveAttribute("aria-valuetext", "Question 1 of 2");
	await question.getByRole("radio", { name: "Fix it here" }).focus();
	await page.keyboard.press("Space");
	await expect(question.getByRole("radio", { name: "Fix it here" })).toBeChecked();
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect(question.getByRole("group", { name: "Which tests should I run?" })).toBeFocused();
	await expect(question.getByRole("progressbar")).toHaveAttribute(
		"aria-valuetext",
		"Question 2 of 2",
	);
	await question
		.getByRole("textbox", { name: "Answer in your own words" })
		.fill("The rounding regression");
	await question.getByRole("button", { name: "Previous", exact: true }).click();
	await expect(question.getByRole("radio", { name: "Fix it here" })).toBeChecked();
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect(question.getByRole("textbox", { name: "Answer in your own words" })).toHaveValue(
		"The rounding regression",
	);
	await question.getByRole("button", { name: "Send answer" }).click();
	await expect(question.getByRole("status")).toHaveText(
		"Answered by jack: Fix it here; The rounding regression",
	);
	await expect(question.getByRole("status")).toBeFocused();
});

test("a Question accepts a free-text answer from the keyboard and ignores whitespace", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html");
	const question = page.getByRole("region", { name: "Question", exact: true });
	const input = question.getByRole("textbox", { name: "Answer in your own words" });
	await expect(question.getByRole("button", { name: "Send answer" })).toBeDisabled();
	await input.fill("   ");
	await expect(question.getByRole("button", { name: "Send answer" })).toBeDisabled();
	await input.fill("  Add a regression test first.  ");
	await input.press("Enter");
	await expect(question.getByRole("status")).toHaveText(
		"Answered by jack: Add a regression test first.",
	);
	await expect(question.getByRole("status")).toBeFocused();
});

test("a one-part Question offers numbered buttons and announces its answer without losing focus", async ({
	page,
}) => {
	await page.goto("/component-tests/approval-question.html");
	const question = page.getByRole("region", { name: "Question", exact: true });
	await expect(
		question.getByRole("heading", { name: "What should I do about the rounding?" }),
	).toBeVisible();
	await expect(question.getByRole("button", { name: "1 Fix it here", exact: true })).toBeVisible();
	await expect(
		question.getByRole("button", { name: "2 Open an issue", exact: true }),
	).toBeVisible();
	await expect(question.getByRole("button", { name: "3 Leave it", exact: true })).toBeVisible();
	await question.getByRole("button", { name: "1 Fix it here", exact: true }).focus();
	await page.keyboard.press("Enter");
	await expect(question.getByRole("status")).toHaveText("Answered by jack: Fix it here");
	await expect(question.getByRole("status")).toBeFocused();
	await expect(question.getByRole("button")).toHaveCount(0);
});
