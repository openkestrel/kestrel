import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Confirmation, type ApprovalState } from "#/components/ai-elements/confirmation";
import { Question, type QuestionState } from "#/components/ui/question";
// oxlint-disable-next-line import/no-unassigned-import -- stories use the application's shared fonts and theme.
import "#/styles.css";

function Stories() {
	const params = new URLSearchParams(location.search);
	const multipart = params.has("multipart");
	const [state, setState] = useState<ApprovalState>(() => {
		switch (params.get("approval")) {
			case "allowed":
				return { status: "resolved", decision: "allow", answeredBy: "sam" };
			case "rejected":
				return { status: "resolved", decision: "reject", answeredBy: "sam" };
			case "refused":
				return { status: "refused", reason: "Only jack or sam may answer this Approval." };
			case "expired":
				return { status: "expired" };
			default:
				return { status: "pending" };
		}
	});
	const [answer, setAnswer] = useState<QuestionState>(() => {
		switch (params.get("question")) {
			case "responded":
				return {
					status: "responded",
					answeredBy: "sam",
					answers: { next: "Open an issue", tests: "CI" },
				};
			case "expired":
				return { status: "expired" };
			default:
				return { status: "pending" };
		}
	});
	return (
		<main className="mx-auto grid max-w-2xl gap-6 p-4">
			<h1 className="text-lg font-medium">Approval and Question fixtures</h1>
			<p className="text-xs text-muted-foreground">
				Examples only. No Session or operator requests.
			</p>
			<Confirmation
				action="Push the branch"
				arguments="git push --force-with-lease origin jack/flaky-usage"
				policy="protected-push"
				mayAnswer={["jack", "sam"]}
				deadline="12 October 2026, 14:30 UTC"
				state={state}
				onAnswer={(decision) =>
					setState(
						params.has("refuse-answer")
							? { status: "refused", reason: "Only jack or sam may answer this Approval." }
							: { status: "resolved", decision, answeredBy: "jack" },
					)
				}
			/>
			<Question
				parts={
					multipart
						? [
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
							]
						: [
								{
									name: "next",
									prompt: "What should I do about the rounding?",
									options: ["Fix it here", "Open an issue", "Leave it"],
								},
							]
				}
				deadline="12 October 2026, 14:30 UTC"
				state={answer}
				onAnswer={(answers) => setAnswer({ status: "responded", answers, answeredBy: "jack" })}
			/>
		</main>
	);
}

createRoot(document.getElementById("root")!).render(<Stories />);
