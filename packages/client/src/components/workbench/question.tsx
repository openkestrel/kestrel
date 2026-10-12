import { useEffect, useId, useRef, useState } from "react";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import {
	Questionnaire,
	QuestionnaireActions,
	QuestionnaireChoice,
	QuestionnaireChoices,
	QuestionnaireError,
	QuestionnaireInput,
	QuestionnaireItem,
	QuestionnaireNext,
	QuestionnairePrevious,
	QuestionnaireProgress,
	QuestionnaireSubmit,
	QuestionnaireTitle,
} from "#/components/ui/questionnaire";

// These are fixture presentation states, not an operator response contract.
export type QuestionState =
	| { status: "pending" }
	| { status: "responded"; answers: Readonly<Record<string, string>>; answeredBy: string }
	| { status: "expired" };

export interface QuestionPart {
	name: string;
	prompt: string;
	options: readonly string[];
}

export interface QuestionProps {
	parts: readonly [QuestionPart, ...QuestionPart[]];
	deadline: string;
	state: QuestionState;
	onAnswer: (answers: Record<string, string>) => void;
}

export function Question({ parts, deadline, state, onAnswer }: QuestionProps) {
	const title = useId();
	const result = useRef<HTMLOutputElement>(null);
	const previous = useRef(state.status);
	const [writtenAnswer, setWrittenAnswer] = useState("");
	const input = useId();
	const [choices, setChoices] = useState<Record<string, string>>({});
	const [written, setWritten] = useState<Record<string, string>>({});
	useEffect(() => {
		if (previous.current !== state.status) result.current?.focus();
		previous.current = state.status;
	}, [state.status]);
	const part = parts[0];
	return (
		<section aria-label="Question" className="grid min-w-0 gap-2 border p-3">
			<h2 id={title} className="text-sm font-medium">
				{parts.length === 1 ? part.prompt : "Question"}
			</h2>
			<output
				ref={result}
				tabIndex={-1}
				className="text-xs text-muted-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring"
			>
				{state.status === "pending" &&
					`Deadline: ${deadline}. On expiry, the agent proceeds on its own judgment.`}
				{state.status === "responded" &&
					`Answered by ${state.answeredBy}: ${parts.map((item) => state.answers[item.name]).join("; ")}`}
				{state.status === "expired" &&
					"Question expired without an answer. The agent proceeds on its own judgment."}
			</output>
			{state.status === "pending" && parts.length > 1 && (
				<Questionnaire
					aria-labelledby={title}
					items={parts.map((item) => ({
						name: item.name,
						required: true,
						choices: item.options.map((value) => ({ value })),
					}))}
					onSubmit={(event) => {
						event.preventDefault();
						const answers = Object.fromEntries(
							parts.map((item) => [
								item.name,
								written[item.name]?.trim() || choices[item.name] || "",
							]),
						);
						if (Object.values(answers).every(Boolean)) onAnswer(answers);
					}}
				>
					<QuestionnaireProgress />
					{parts.map((item, index) => (
						<QuestionnaireItem key={item.name} name={item.name} required>
							<QuestionnaireTitle>{item.prompt}</QuestionnaireTitle>
							<QuestionnaireChoices>
								{item.options.map((option) => (
									<QuestionnaireChoice
										key={option}
										value={option}
										checked={choices[item.name] === option}
										onChange={() => {
											setChoices((values) => ({ ...values, [item.name]: option }));
											setWritten((values) => ({ ...values, [item.name]: "" }));
										}}
									>
										{option}
									</QuestionnaireChoice>
								))}
							</QuestionnaireChoices>
							<label htmlFor={`${input}-${index}`} className="text-xs text-muted-foreground">
								Answer in your own words
							</label>
							<QuestionnaireInput
								id={`${input}-${index}`}
								value={written[item.name] ?? ""}
								onChange={(event) => {
									setWritten((values) => ({ ...values, [item.name]: event.target.value }));
									setChoices((values) => ({ ...values, [item.name]: "" }));
								}}
							/>
							<QuestionnaireError />
						</QuestionnaireItem>
					))}
					<QuestionnaireActions>
						<QuestionnairePrevious />
						<QuestionnaireNext />
						<QuestionnaireSubmit>Send answer</QuestionnaireSubmit>
					</QuestionnaireActions>
				</Questionnaire>
			)}
			{state.status === "pending" && parts.length === 1 && (
				<form
					aria-labelledby={title}
					className="flex flex-wrap gap-2"
					onSubmit={(event) => {
						event.preventDefault();
						if (writtenAnswer.trim()) onAnswer({ [part.name]: writtenAnswer.trim() });
					}}
				>
					{part.options.map((option, index) => (
						<Button
							key={option}
							variant="outline"
							type="button"
							className="min-h-11 sm:min-h-8"
							onClick={() => onAnswer({ [part.name]: option })}
						>
							<span className="text-muted-foreground">{index + 1}</span> {option}
						</Button>
					))}
					<div className="flex w-full min-w-0 flex-wrap gap-2">
						<label htmlFor={input} className="w-full text-xs text-muted-foreground">
							Answer in your own words
						</label>
						<Input
							id={input}
							value={writtenAnswer}
							onChange={(event) => setWrittenAnswer(event.target.value)}
							className="min-h-11 flex-1 sm:min-h-8"
						/>
						<Button type="submit" disabled={!writtenAnswer.trim()} className="min-h-11 sm:min-h-8">
							Send answer
						</Button>
					</div>
				</form>
			)}
		</section>
	);
}
