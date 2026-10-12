import { useEffect, useState } from "react";

export function useNow(every: number): number {
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		const tick = setInterval(() => setNow(Date.now()), every);
		return () => clearInterval(tick);
	}, [every]);
	return now;
}
