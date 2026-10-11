import { useEffect, useState } from "react";

const DELAYED_AFTER_MS = 2_000;

export function useDelayed(pending: boolean): boolean {
	const [delayed, setDelayed] = useState(false);

	useEffect(() => {
		if (!pending) return undefined;
		const timer = setTimeout(() => setDelayed(true), DELAYED_AFTER_MS);
		return () => {
			clearTimeout(timer);
			setDelayed(false);
		};
	}, [pending]);

	return pending && delayed;
}
