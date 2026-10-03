import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { cacheWarning, optionChange } from "#/operator/composer";
import type { Session, SessionOption, Usage } from "#/operator/generated";
import { useParticipant } from "#/operator/participant";
import { operator, sessionsKey } from "#/operator/queries";
import { changeSessionOption } from "#/operator/turns";

export type OptionChoice = { option: SessionOption; value: string; applied?: () => void };

export type OptionWrite = ReturnType<typeof useOptionWrite>;

export function useOptionWrite(
	organization: string,
	session: Session | undefined,
	usage: Usage | null | undefined,
) {
	const name = useParticipant();
	const queryClient = useQueryClient();
	const [refusal, setRefusal] = useState<unknown>(null);
	const [confirming, setConfirming] = useState<OptionChoice | null>(null);
	const [writing, setWriting] = useState(false);
	// State lags a second keypress in the same frame; the ref does not.
	const inFlight = useRef(false);

	async function write(choice: OptionChoice) {
		if (!session || !name || inFlight.current) return;
		inFlight.current = true;
		setWriting(true);
		setRefusal(null);
		try {
			await changeSessionOption(
				operator,
				organization,
				session.id,
				optionChange(choice.option, choice.value, name),
			);
			setConfirming(null);
			choice.applied?.();
			await queryClient.invalidateQueries({ queryKey: sessionsKey(organization) });
		} catch (error) {
			setRefusal(error);
		} finally {
			inFlight.current = false;
			setWriting(false);
		}
	}

	function choose(choice: OptionChoice): string | undefined {
		const warning = cacheWarning(choice.option, usage);
		if (warning) {
			setConfirming(choice);
			return warning;
		}
		void write(choice);
		return undefined;
	}

	return {
		refusal,
		confirming,
		warning: confirming ? cacheWarning(confirming.option, usage) : undefined,
		writing,
		inFlight: () => inFlight.current,
		choose,
		confirm: () => confirming && void write(confirming),
		cancel: () => setConfirming(null),
	};
}
