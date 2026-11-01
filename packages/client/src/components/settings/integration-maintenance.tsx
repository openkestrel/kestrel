import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { described, Field, placement } from "#/components/field";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Input } from "#/components/ui/input";
import type { Integration } from "#/operator/generated";
import {
	consequenceOf,
	maintain,
	type Maintenance,
	offered,
	replacePrivateKey,
	replaced,
} from "#/operator/integrations";
import { integrationsQuery } from "#/operator/repositories";

const OFFER: Record<Maintenance, string> = {
	pause: "Pause",
	key: "Replace the App private key",
	rotate: "Rotate the webhook secret",
	retire: "Retire",
};

const DONE: Record<Maintenance, string> = {
	pause: "Paused. Resume it whenever you like; nothing it resumes with was erased.",
	key: "Replaced the App private key. It still signs as the same App, for the same installation and repository.",
	rotate: "GitHub acknowledged the new webhook secret, which is now the only one that verifies.",
	retire: "Retired for good in kestrel. The GitHub App and its installation are untouched.",
};

export function IntegrationMaintenance({
	organization,
	integration,
	retry,
}: {
	organization: string;
	integration: Integration;
	retry: () => void;
}) {
	const queryClient = useQueryClient();
	const [chosen, setChosen] = useState<Maintenance | null>(null);
	const [done, setDone] = useState<Maintenance>();
	const offers = offered(integration);

	async function settle(changed: Integration, kind: Maintenance) {
		queryClient.setQueryData(integrationsQuery(organization).queryKey, (listed) =>
			replaced(listed, changed),
		);
		setChosen(null);
		setDone(kind);
		await queryClient.invalidateQueries({ queryKey: ["organizations", organization] });
		await queryClient.invalidateQueries({ queryKey: ["readiness"] });
	}

	const writing = useMutation({
		mutationFn: (kind: Exclude<Maintenance, "key">) => maintain(organization, integration, kind),
		onSuccess: (changed, kind) => settle(changed, kind),
		// A refused write may still have moved the record, as an unacknowledged rotation does.
		onError: () => queryClient.invalidateQueries({ queryKey: ["organizations", organization] }),
	});

	function choose(kind: Maintenance | null) {
		writing.reset();
		setDone(undefined);
		setChosen(kind);
	}

	if (offers.length === 0 && done === undefined) return null;
	return (
		<section aria-label={`Maintain ${integration.name}`} className="grid gap-2" data-maintenance>
			{offers.length > 0 && (
				<div className="flex flex-wrap gap-2">
					{offers.map((kind) => (
						<Button
							key={kind}
							size="xs"
							type="button"
							variant={kind === "retire" ? "destructive" : "outline"}
							aria-expanded={chosen === kind}
							disabled={writing.isPending}
							onClick={() => choose(chosen === kind ? null : kind)}
						>
							{OFFER[kind]}…
						</Button>
					))}
				</div>
			)}
			{chosen === "key" && (
				<KeyReplacement
					organization={organization}
					integration={integration}
					onReplaced={(changed) => void settle(changed, "key")}
					onCancel={() => choose(null)}
				/>
			)}
			{chosen !== null && chosen !== "key" && offers.includes(chosen) && (
				<Confirmation
					kind={chosen}
					integration={integration}
					pending={writing.isPending}
					onConfirm={() => writing.mutate(chosen)}
					onCancel={() => choose(null)}
				/>
			)}
			{writing.isError && <Refusal error={writing.error} retry={retry} />}
			{done && (
				<output className="text-xs" data-maintained={done}>
					{DONE[done]}
				</output>
			)}
		</section>
	);
}

function Confirmation({
	kind,
	integration,
	pending,
	onConfirm,
	onCancel,
}: {
	kind: Exclude<Maintenance, "key">;
	integration: Integration;
	pending: boolean;
	onConfirm: () => void;
	onCancel: () => void;
}) {
	const [typed, setTyped] = useState("");
	const id = `retire-${integration.id}`;
	const retiring = kind === "retire";
	const confirmed = !retiring || typed === integration.name;
	return (
		<fieldset className="grid gap-2 border p-3 text-xs" data-confirm={kind}>
			<legend className="px-1 font-semibold">
				{OFFER[kind]} {integration.name}
			</legend>
			<p data-consequence>{consequenceOf(kind, integration)}</p>
			{retiring && (
				<Field label={`Type ${integration.name} to retire it`} htmlFor={id}>
					<Input
						id={id}
						value={typed}
						autoComplete="off"
						spellCheck={false}
						onChange={(event) => setTyped(event.target.value)}
					/>
				</Field>
			)}
			<div className="flex gap-2">
				<Button
					size="xs"
					type="button"
					variant={retiring ? "destructive" : "default"}
					disabled={pending || !confirmed}
					onClick={onConfirm}
				>
					{pending ? "Sending…" : `${OFFER[kind]} ${integration.name}`}
				</Button>
				<Button size="xs" type="button" variant="ghost" onClick={onCancel}>
					Cancel
				</Button>
			</div>
		</fieldset>
	);
}

// The key lives only in this component's state while it is chosen, never in a draft or the cache.
function KeyReplacement({
	organization,
	integration,
	onReplaced,
	onCancel,
}: {
	organization: string;
	integration: Integration;
	onReplaced: (changed: Integration) => void;
	onCancel: () => void;
}) {
	const [key, setKey] = useState<string>();
	const replacing = useMutation({
		mutationFn: (pem: string) => replacePrivateKey(organization, integration, pem),
		onSuccess: (changed) => {
			setKey(undefined);
			onReplaced(changed);
		},
	});
	const id = `private-key-${integration.id}`;
	const { placed, atTheTop, repair } = placement(replacing.error, {}, { private_key: id });
	const refused = placed("private_key") !== undefined;

	return (
		<form
			aria-label={`Replace ${integration.name}'s App private key`}
			className="grid gap-2 border p-3 text-xs"
			data-key-replacement
			onSubmit={(event) => {
				event.preventDefault();
				if (key !== undefined) replacing.mutate(key);
			}}
		>
			<p data-consequence>{consequenceOf("key", integration)}</p>
			{atTheTop && <Refusal error={replacing.error} />}
			<Field
				label="Replacement private key"
				htmlFor={id}
				error={placed("private_key")}
				repair={repair("private_key")}
				hint="The App's .pem file from GitHub. It is sent once and never shown or returned."
			>
				<Input
					id={id}
					type="file"
					accept=".pem,application/x-pem-file"
					aria-invalid={refused}
					aria-describedby={described(id, true, refused)}
					onChange={(event) => {
						replacing.reset();
						const file = event.target.files?.[0];
						if (file === undefined) setKey(undefined);
						else void file.text().then(setKey);
					}}
				/>
			</Field>
			{replacing.isError && <p data-key-kept>Nothing was saved; the current key stays in use.</p>}
			<div className="flex gap-2">
				<Button size="xs" type="submit" disabled={key === undefined || replacing.isPending}>
					{replacing.isPending ? "Asking GitHub…" : "Replace the key"}
				</Button>
				<Button
					size="xs"
					type="button"
					variant="ghost"
					onClick={() => {
						setKey(undefined);
						onCancel();
					}}
				>
					Cancel
				</Button>
			</div>
		</form>
	);
}
