import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { Button } from "#/components/ui/button";
import { type Outage, outageOf, untilAttempt } from "#/operator/outage";
import { operator } from "#/operator/queries";
import { operatorPath } from "#/operator/transport";

// The page stays mounted under the outage, so what someone typed survives until reads resume.
export function OutageGuard({ children }: { children: ReactNode }) {
	const queryClient = useQueryClient();
	const [outage, setOutage] = useState<Outage>();

	useEffect(
		() =>
			queryClient.getQueryCache().subscribe((event) => {
				if (event.type !== "updated" || event.action.type !== "error") return;
				const found = outageOf(event.action.error);
				if (found) setOutage(found);
			}),
		[queryClient],
	);

	const recovered = useCallback(() => {
		setOutage(undefined);
		void queryClient.invalidateQueries();
	}, [queryClient]);

	return (
		<>
			{outage && <OutagePage outage={outage} recovered={recovered} />}
			<div hidden={outage !== undefined}>{children}</div>
		</>
	);
}

function OutagePage({ outage, recovered }: { outage: Outage; recovered: () => void }) {
	const [{ attempt, left }, setSchedule] = useState(scheduled(0));
	const [checking, setChecking] = useState(false);

	const check = useCallback(async () => {
		setChecking(true);
		try {
			await operator.read(operatorPath("organizations"));
			recovered();
		} catch (error) {
			// Any other answer came from a running control plane, and its own page explains it.
			if (outageOf(error) === undefined) recovered();
			else {
				setSchedule((was) => scheduled(was.attempt + 1));
				setChecking(false);
			}
		}
	}, [recovered]);

	useEffect(() => {
		if (checking || left === undefined) return undefined;
		const tick = setTimeout(
			() => (left <= 1 ? void check() : setSchedule({ attempt, left: left - 1 })),
			1000,
		);
		return () => clearTimeout(tick);
	}, [attempt, check, checking, left]);

	return (
		<main className="mx-auto grid max-w-xl gap-4 p-8" data-outage>
			<h1 className="font-semibold text-lg">kestrel isn't running</h1>
			<p className="text-sm">
				The browser Client is up, but the control plane behind {outage.origin} isn't answering. This
				page carries on once it does.
			</p>
			{outage.compose ? (
				<div className="grid gap-2 text-sm">
					<p>See whether its services are up, and what the control plane said before it stopped:</p>
					<pre className="bg-muted p-2">
						<code>docker compose ps</code>
					</pre>
					<pre className="bg-muted p-2">
						<code>docker compose logs kestrel</code>
					</pre>
				</div>
			) : (
				<p className="text-sm">Check that it is running and reachable from this browser.</p>
			)}
			<div className="flex items-center gap-2 text-muted-foreground text-sm">
				<p aria-live="polite">
					{checking
						? "Checking…"
						: left === undefined
							? `Stopped trying after ${attempt} attempts.`
							: `Trying again in ${left} s.`}
				</p>
				<Button
					size="xs"
					type="button"
					variant="outline"
					disabled={checking}
					onClick={() => {
						if (left === undefined) setSchedule(scheduled(0));
						void check();
					}}
				>
					{left === undefined ? "Try again" : "Try now"}
				</Button>
			</div>
		</main>
	);
}

function scheduled(attempt: number): { attempt: number; left: number | undefined } {
	const wait = untilAttempt(attempt);
	return { attempt, left: wait === undefined ? undefined : wait / 1000 };
}
