import { useState } from "react";
import {
	Context,
	ContextTrigger,
	ContextContent,
	ContextContentHeader,
	ContextContentFooter,
} from "#/components/ai-elements/context";
import { ModelChoice } from "#/components/model-choice";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { mayWriteOptions, nameToChangeOptions, optionValues } from "#/operator/composer";
import type { Session, Usage } from "#/operator/generated";
import { changingModel, modelOption } from "#/operator/model-choice";
import { useParticipant } from "#/operator/participant";
import { optionCurrent, pendingLine, usageLine } from "#/operator/session-view";
import type { OptionWrite } from "./option-write";

export function SessionControls({
	session,
	usage,
	optionWrite,
}: {
	session: Session | undefined;
	usage: Usage | null | undefined;
	optionWrite: OptionWrite;
}) {
	const name = useParticipant();
	const options = session?.options ?? [];
	const canWrite = mayWriteOptions(session?.state) && name !== null;
	const model = modelOption(options);
	const changing = changingModel(session);
	const [contextOpen, setContextOpen] = useState(false);
	const percent =
		usage && usage.context_size > 0
			? new Intl.NumberFormat("en-US", { style: "percent", maximumFractionDigits: 1 }).format(
					usage.context_used / usage.context_size,
				)
			: "0%";
	return (
		// oxlint-disable jsx-a11y/prefer-tag-over-role -- A fieldset's box and legend would fight the footer's flex row; only a name is wanted.
		<div
			className="flex w-full min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs"
			role="group"
			aria-label="Session controls"
		>
			{usage && (
				<Context
					usedTokens={usage.context_used}
					maxTokens={usage.context_size}
					usage={usage}
					open={contextOpen}
					onOpenChange={setContextOpen}
				>
					<ContextTrigger
						aria-label={`Context: ${percent}, ${usageLine(usage)}`}
						onClick={() => setContextOpen(true)}
					/>
					<ContextContent align="start">
						<ContextContentHeader>
							<p>{percent}</p>
							<p data-session-usage>
								{usage.context_used.toLocaleString()} of {usage.context_size.toLocaleString()}{" "}
								tokens
							</p>
						</ContextContentHeader>
						<ContextContentFooter />
					</ContextContent>
				</Context>
			)}
			{options.length > 0 && (
				<div className="flex flex-wrap items-center gap-2" data-session-options>
					{options.map((option) => (
						<div
							key={option.id}
							className="flex flex-wrap items-center gap-1 text-xs"
							data-option={option.id}
						>
							{option !== model && <span className="text-muted-foreground">{option.name}:</span>}
							{option === model ? (
								<ModelChoice
									id="session-model"
									offered={option}
									value={String(option.current)}
									disabled={!canWrite || optionWrite.writing || changing !== undefined}
									onChange={(value) => optionWrite.choose({ option, value })}
								/>
							) : (
								<>
									<span data-option-current>{optionCurrent(option)}</span>
									{optionValues(option)
										.filter((value) => value.value !== String(option.current))
										.map((value) => (
											<Button
												key={value.value}
												data-option-value={value.value}
												disabled={!canWrite || optionWrite.writing}
												onClick={() => optionWrite.choose({ option, value: value.value })}
												size="xs"
												type="button"
												variant="outline"
											>
												{value.name}
											</Button>
										))}
								</>
							)}
							{option.kind === "boolean" && (
								<Button
									data-option-toggle
									disabled={!canWrite || optionWrite.writing}
									onClick={() =>
										optionWrite.choose({ option, value: option.current ? "false" : "true" })
									}
									size="xs"
									type="button"
									variant="outline"
								>
									{option.current ? "turn off" : "turn on"}
								</Button>
							)}
						</div>
					))}
					{session?.changing_options.map((change) => (
						<p
							key={`${change.option}:${change.participant}`}
							className="text-muted-foreground text-xs"
							data-changing-option
						>
							{pendingLine(change)}
						</p>
					))}
					{!name && (
						<p className="text-muted-foreground text-xs" data-option-note>
							{nameToChangeOptions}
						</p>
					)}
				</div>
			)}
			{optionWrite.confirming && (
				<div className="mt-2 border border-border p-2 text-xs" data-option-confirm>
					<p>{optionWrite.warning}</p>
					<p className="text-muted-foreground">
						Change {optionWrite.confirming.option.name} to {optionWrite.confirming.value}?
					</p>
					<div className="mt-1 flex gap-1">
						<Button
							disabled={optionWrite.writing}
							onClick={optionWrite.confirm}
							size="xs"
							type="button"
						>
							Change
						</Button>
						<Button onClick={optionWrite.cancel} size="xs" type="button" variant="ghost">
							Cancel
						</Button>
					</div>
				</div>
			)}
			{optionWrite.refusal !== null && (
				<div className="mt-2">
					<Refusal error={optionWrite.refusal} />
				</div>
			)}
		</div>
		// oxlint-enable jsx-a11y/prefer-tag-over-role
	);
}
