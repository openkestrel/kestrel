import { ChevronsUpDown } from "lucide-react";
import { useState } from "react";
import {
	ModelSelector,
	ModelSelectorContent,
	ModelSelectorEmpty,
	ModelSelectorGroup,
	ModelSelectorInput,
	ModelSelectorItem,
	ModelSelectorList,
	ModelSelectorLogo,
	ModelSelectorName,
	ModelSelectorTrigger,
} from "#/components/ai-elements/model-selector";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import type { SessionOption } from "#/operator/generated";
import {
	HARNESS_DEFAULT,
	offeredGroups,
	offeredMatch,
	offeredName,
	overrideLine,
	providerOf,
} from "#/operator/model-choice";

type Described = { "aria-invalid"?: boolean; "aria-describedby"?: string };

export function ModelChoice({
	id,
	offered,
	value,
	onChange,
	disabled = false,
	fallback = HARNESS_DEFAULT,
	...aria
}: {
	id: string;
	offered?: SessionOption;
	value: string;
	onChange: (value: string) => void;
	disabled?: boolean;
	fallback?: string;
} & Described) {
	return offered ? (
		<OfferedModels
			id={id}
			offered={offered}
			value={value}
			onChange={onChange}
			disabled={disabled}
		/>
	) : (
		<ModelOverride
			id={id}
			value={value}
			onChange={onChange}
			disabled={disabled}
			fallback={fallback}
			{...aria}
		/>
	);
}

function Mark({
	value,
	group,
	className,
}: {
	value: string;
	group?: string | null;
	className?: string;
}) {
	const provider = providerOf(value, group);
	return provider ? <ModelSelectorLogo provider={provider} className={className} /> : null;
}

function OfferedModels({
	id,
	offered,
	value,
	onChange,
	disabled,
}: {
	id: string;
	offered: SessionOption;
	value: string;
	onChange: (value: string) => void;
	disabled: boolean;
}) {
	const [open, setOpen] = useState(false);
	const groups = offeredGroups(offered);
	const current = groups.find((group) => group.values.some((model) => model.value === value));

	return (
		<ModelSelector open={open} onOpenChange={setOpen}>
			<ModelSelectorTrigger
				id={id}
				data-model-choice="offered"
				aria-label={`${offered.name}: ${offeredName(offered, value)}`}
				disabled={disabled}
				render={<Button size="xs" type="button" variant="outline" />}
			>
				<Mark value={value} group={current?.group} />
				<ModelSelectorName>{offeredName(offered, value)}</ModelSelectorName>
				<ChevronsUpDown aria-hidden />
			</ModelSelectorTrigger>
			<ModelSelectorContent
				title="Choose a model the harness offers"
				filter={offeredMatch}
				showCloseButton={false}
			>
				<ModelSelectorInput placeholder="Search the offered models…" />
				<ModelSelectorList>
					<ModelSelectorEmpty>The harness offers no model matching that.</ModelSelectorEmpty>
					{groups.map((group) => (
						<ModelSelectorGroup key={group.group ?? ""} heading={group.name ?? undefined}>
							{group.values.map((model) => (
								<ModelSelectorItem
									key={model.value}
									value={model.value}
									keywords={[model.name, group.name ?? ""]}
									data-checked={model.value === value}
									onSelect={() => {
										setOpen(false);
										if (model.value !== value) onChange(model.value);
									}}
								>
									<Mark value={model.value} group={group.group} />
									<ModelSelectorName>{model.name}</ModelSelectorName>
									{model.description && (
										<span className="truncate text-muted-foreground">{model.description}</span>
									)}
								</ModelSelectorItem>
							))}
						</ModelSelectorGroup>
					))}
				</ModelSelectorList>
			</ModelSelectorContent>
		</ModelSelector>
	);
}

function ModelOverride({
	id,
	value,
	onChange,
	disabled,
	fallback,
	...aria
}: {
	id: string;
	value: string;
	onChange: (value: string) => void;
	disabled: boolean;
	fallback: string;
} & Described) {
	const overriding = value.trim() !== "";

	return (
		<div className="grid gap-1.5" data-model-choice="override">
			<Input
				id={id}
				value={value}
				disabled={disabled}
				placeholder={fallback}
				autoComplete="off"
				spellCheck={false}
				onChange={(event) => onChange(event.target.value)}
				{...aria}
			/>
			<p className="flex items-start gap-1.5 text-muted-foreground text-xs" data-model-line>
				{overriding && <Mark value={value.trim()} className="mt-0.5" />}
				{overrideLine(value, fallback)}
			</p>
			{overriding && (
				<Button
					className="justify-self-start"
					size="xs"
					type="button"
					variant="outline"
					disabled={disabled}
					onClick={() => onChange("")}
				>
					Use {fallback}
				</Button>
			)}
		</div>
	);
}
