import axe from "axe-core";
import { useState } from "react";
import { expect, test } from "vitest";
// oxlint-disable-next-line import/no-unassigned-import -- the fixtures render with the application's fonts and theme.
import "#/styles.css";
import { render } from "vitest-browser-react";
import { cdp, page, userEvent } from "vitest/browser";
import type { ApprovalState } from "#/components/ai-elements/confirmation";
import { Approval } from "#/components/workbench/approval";
import { Question, type QuestionPart, type QuestionState } from "#/components/workbench/question";

interface FixtureProps {
	approval?: ApprovalState;
	question?: QuestionState;
	multipart?: boolean;
	refuseAnswer?: boolean;
}

const parts: readonly [QuestionPart, ...QuestionPart[]] = [
	{
		name: "next",
		prompt: "What should I do about the rounding?",
		options: ["Fix it here", "Open an issue", "Leave it"],
	},
	{
		name: "tests",
		prompt: "Which tests should I run?",
		options: ["Focused tests", "CI"],
	},
];

const pendingApproval: ApprovalState = { status: "pending" };
const pendingQuestion: QuestionState = { status: "pending" };

// The browser-mode iframe starts unfocused, so a click gives it focus for the keyboard tests.
async function renderFixtures(props: FixtureProps = {}) {
	const view = await render(<Fixture {...props} />);
	await view.getByRole("heading", { name: "Approval and Question fixtures" }).click();
	return view;
}

function Fixture({
	approval = pendingApproval,
	question = pendingQuestion,
	multipart = false,
	refuseAnswer = false,
}: FixtureProps) {
	const [approvalState, setApprovalState] = useState(approval);
	const [questionState, setQuestionState] = useState(question);
	return (
		<main className="mx-auto grid max-w-2xl gap-6 p-4">
			<h1 className="text-lg font-medium">Approval and Question fixtures</h1>
			<p className="text-xs text-muted-foreground">
				Examples only. No Session or operator requests.
			</p>
			<Approval
				action="Push the branch"
				arguments="git push --force-with-lease origin jack/flaky-usage"
				policy="protected-push"
				mayAnswer={["jack", "sam"]}
				deadline="12 October 2026, 14:30 UTC"
				state={approvalState}
				onAnswer={(decision) =>
					setApprovalState(
						refuseAnswer
							? { status: "refused", reason: "Only jack or sam may answer this Approval." }
							: { status: "resolved", decision, answeredBy: "jack" },
					)
				}
			/>
			<Question
				parts={multipart ? parts : [parts[0]]}
				deadline="12 October 2026, 14:30 UTC"
				state={questionState}
				onAnswer={(answers) =>
					setQuestionState({ status: "responded", answers, answeredBy: "jack" })
				}
			/>
		</main>
	);
}

test("an Approval names the action, Policy and answerers and can be allowed from the keyboard", async () => {
	const view = await renderFixtures();
	const approval = view.getByRole("region", { name: "Approval: Push the branch" });
	await expect
		.element(approval)
		.toMatchTextContent("git push --force-with-lease origin jack/flaky-usage");
	await expect.element(approval).toMatchTextContent("Policy protected-push");
	await expect.element(approval).toMatchTextContent("jack or sam may answer");
	await expect
		.element(approval.getByRole("status"))
		.toHaveTextContent("Pending · Deadline: 12 October 2026, 14:30 UTC");
	await userEvent.tab();
	await expect.element(approval.getByRole("button", { name: "Reject", exact: true })).toHaveFocus();
	await userEvent.tab();
	await expect.element(approval.getByRole("button", { name: "Allow", exact: true })).toHaveFocus();
	await userEvent.keyboard("{Enter}");
	await expect.element(approval.getByRole("status")).toHaveTextContent("Allowed by jack.");
	await expect.element(approval.getByRole("status")).toHaveFocus();
	expect(approval.getByRole("button").elements()).toHaveLength(0);
});

test("Reject resolves an Approval, while a refused answer reports the refusal", async () => {
	const view = await renderFixtures();
	const approval = view.getByRole("region", { name: "Approval: Push the branch" });
	approval.getByRole("button", { name: "Reject", exact: true }).element().focus();
	await userEvent.keyboard("{Enter}");
	await expect.element(approval.getByRole("status")).toHaveTextContent("Rejected by jack.");
	await expect.element(approval.getByRole("status")).toHaveFocus();
	await view.unmount();
	const refused = await renderFixtures({ refuseAnswer: true });
	const refusedApproval = refused.getByRole("region", { name: "Approval: Push the branch" });
	refusedApproval.getByRole("button", { name: "Allow", exact: true }).element().focus();
	await userEvent.keyboard("{Enter}");
	await expect
		.element(refusedApproval.getByRole("alert"))
		.toHaveTextContent("Answer refused: Only jack or sam may answer this Approval.");
	await expect.element(refusedApproval.getByRole("alert")).toHaveFocus();
	expect(refusedApproval.getByRole("button").elements()).toHaveLength(0);
});

test("Question expiry leaves judgment to the agent, while Approval expiry denies the action", async () => {
	const view = await renderFixtures({
		approval: { status: "expired" },
		question: { status: "expired" },
	});
	const question = view.getByRole("region", { name: "Question", exact: true });
	await expect
		.element(question.getByRole("status"))
		.toHaveTextContent(
			"Question expired without an answer. The agent proceeds on its own judgment.",
		);
	expect(question.getByRole("button").elements()).toHaveLength(0);
	expect(question.getByRole("textbox").elements()).toHaveLength(0);
	const approval = view.getByRole("region", { name: "Approval: Push the branch" });
	await expect
		.element(approval.getByRole("status"))
		.toHaveTextContent("Approval expired. The action was denied.");
	expect(page.getByRole("button").elements()).toHaveLength(0);
});

test("multipart Questions explain an unanswered part before continuing", async () => {
	const view = await renderFixtures({ multipart: true });
	const question = view.getByRole("region", { name: "Question", exact: true });
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect
		.element(question.getByRole("alert"))
		.toHaveTextContent("Choose an answer to continue.");
	await expect
		.element(question.getByRole("progressbar"))
		.toHaveAttribute("aria-valuetext", "Question 1 of 2");
});

test("a multipart Question keeps its answers while navigating and submits them together", async () => {
	const view = await renderFixtures({ multipart: true });
	const question = view.getByRole("region", { name: "Question", exact: true });
	await expect
		.element(question.getByRole("progressbar", { name: "Questionnaire progress" }))
		.toHaveAttribute("aria-valuetext", "Question 1 of 2");
	question.getByRole("radio", { name: "Fix it here" }).element().focus();
	await userEvent.keyboard(" ");
	await expect.element(question.getByRole("radio", { name: "Fix it here" })).toBeChecked();
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect
		.element(question.getByRole("group", { name: "Which tests should I run?" }))
		.toHaveFocus();
	await expect
		.element(question.getByRole("progressbar"))
		.toHaveAttribute("aria-valuetext", "Question 2 of 2");
	await question
		.getByRole("textbox", { name: "Answer in your own words" })
		.fill("The rounding regression");
	await question.getByRole("button", { name: "Previous", exact: true }).click();
	await expect.element(question.getByRole("radio", { name: "Fix it here" })).toBeChecked();
	await question.getByRole("button", { name: "Next", exact: true }).click();
	await expect
		.element(question.getByRole("textbox", { name: "Answer in your own words" }))
		.toHaveValue("The rounding regression");
	await question.getByRole("button", { name: "Send answer" }).click();
	await expect
		.element(question.getByRole("status"))
		.toHaveTextContent("Answered by jack: Fix it here; The rounding regression");
	await expect.element(question.getByRole("status")).toHaveFocus();
});

test("a Question accepts a free-text answer from the keyboard and ignores whitespace", async () => {
	const view = await renderFixtures();
	const question = view.getByRole("region", { name: "Question", exact: true });
	const input = question.getByRole("textbox", { name: "Answer in your own words" });
	await expect.element(question.getByRole("button", { name: "Send answer" })).toBeDisabled();
	await input.fill("   ");
	await expect.element(question.getByRole("button", { name: "Send answer" })).toBeDisabled();
	await input.fill("  Add a regression test first.  ");
	await userEvent.keyboard("{Enter}");
	await expect
		.element(question.getByRole("status"))
		.toHaveTextContent("Answered by jack: Add a regression test first.");
	await expect.element(question.getByRole("status")).toHaveFocus();
});

test("a one-part Question offers numbered buttons and announces its answer without losing focus", async () => {
	const view = await renderFixtures();
	const question = view.getByRole("region", { name: "Question", exact: true });
	await expect
		.element(question.getByRole("heading", { name: "What should I do about the rounding?" }))
		.toBeVisible();
	await expect
		.element(question.getByRole("button", { name: "1 Fix it here", exact: true }))
		.toBeVisible();
	await expect
		.element(question.getByRole("button", { name: "2 Open an issue", exact: true }))
		.toBeVisible();
	await expect
		.element(question.getByRole("button", { name: "3 Leave it", exact: true }))
		.toBeVisible();
	question.getByRole("button", { name: "1 Fix it here", exact: true }).element().focus();
	await userEvent.keyboard("{Enter}");
	await expect
		.element(question.getByRole("status"))
		.toHaveTextContent("Answered by jack: Fix it here");
	await expect.element(question.getByRole("status")).toHaveFocus();
	expect(question.getByRole("button").elements()).toHaveLength(0);
});

const states: { props: FixtureProps; approval: string; question: string }[] = [
	{
		props: {},
		approval: "Pending",
		question: "On expiry, the agent proceeds on its own judgment.",
	},
	{
		props: {
			approval: { status: "resolved", decision: "allow", answeredBy: "sam" },
			question: {
				status: "responded",
				answeredBy: "sam",
				answers: { next: "Open an issue", tests: "CI" },
			},
		},
		approval: "Allowed by sam.",
		question: "Answered by sam: Open an issue",
	},
	{
		props: {
			approval: { status: "resolved", decision: "reject", answeredBy: "sam" },
			question: { status: "expired" },
		},
		approval: "Rejected by sam.",
		question: "Question expired without an answer.",
	},
	{
		props: {
			approval: { status: "refused", reason: "Only jack or sam may answer this Approval." },
			multipart: true,
		},
		approval: "Answer refused",
		question: "On expiry, the agent proceeds on its own judgment.",
	},
	{
		props: {
			approval: { status: "expired" },
			question: {
				status: "responded",
				answeredBy: "sam",
				answers: { next: "Open an issue", tests: "CI" },
			},
			multipart: true,
		},
		approval: "Approval expired",
		question: "Answered by sam: Open an issue; CI",
	},
];

for (const colorScheme of ["light", "dark"] as const) {
	for (const width of [1280, 375]) {
		test(`fixture states remain accessible at ${width}px in ${colorScheme}`, async () => {
			await page.viewport(width, 800);
			await cdp().send("Emulation.setEmulatedMedia", {
				media: "",
				features: [{ name: "prefers-color-scheme", value: colorScheme }],
			});
			for (const { props, approval: approvalText, question: questionText } of states) {
				// oxlint-disable-next-line no-await-in-loop -- each state is rendered, examined and unmounted in turn.
				const view = await render(<Fixture {...props} />);
				// oxlint-disable-next-line no-await-in-loop -- the state is asserted before axe examines it.
				await expect
					.element(view.getByRole("region", { name: "Approval: Push the branch" }))
					.toMatchTextContent(approvalText);
				// oxlint-disable-next-line no-await-in-loop -- the same state's Question is asserted next.
				await expect
					.element(view.getByRole("region", { name: "Question", exact: true }))
					.toMatchTextContent(questionText);
				// oxlint-disable-next-line no-await-in-loop -- axe examines this rendered state before it is unmounted.
				const { violations } = await axe.run(document.body);
				expect(violations).toEqual([]);
				expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth);
				// oxlint-disable-next-line no-await-in-loop -- each state needs a clean render.
				await view.unmount();
			}
		}, 120_000);
	}
}
