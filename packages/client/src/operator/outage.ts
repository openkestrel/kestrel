import { saysCompose, Unreachable } from "./transport";

export type Outage = { origin: string; compose: boolean };

export const ATTEMPTS = 20;

export function outageOf(error: unknown): Outage | undefined {
	if (!(error instanceof Unreachable)) return undefined;
	const { context } = error.diagnostic;
	return {
		origin: "url" in context ? context.url : location.origin,
		compose: saysCompose(error.diagnostic),
	};
}

export function untilAttempt(attempt: number): number | undefined {
	if (attempt >= ATTEMPTS) return undefined;
	return Math.min(2_000 * 2 ** attempt, 30_000);
}
