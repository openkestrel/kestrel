import { useSyncExternalStore } from "react";

const KEY = "kestrel:participant";
const LONGEST = 64;

export function nameRefusal(name: string): string | null {
	const trimmed = name.trim();
	if (trimmed.length === 0) return "A name is needed before writing";
	if (trimmed.length > LONGEST) return `A name is at most ${LONGEST} characters`;
	for (const character of trimmed) {
		if (character.charCodeAt(0) < 32 || character === "\u007f") {
			return "A name cannot contain a control character";
		}
	}
	return null;
}

type Storage = Pick<globalThis.Storage, "getItem" | "setItem" | "removeItem">;

// Never authentication: the operator boundary's participant rule refuses what this cannot.
export class Participant {
	private readonly storage: Storage | null;
	private readonly listeners = new Set<() => void>();
	private current: string | null;

	constructor(storage: Storage | null = browserStorage()) {
		this.storage = storage;
		this.current = stored(storage);
	}

	name(): string | null {
		return this.current;
	}

	remember(name: string): string | null {
		const refusal = nameRefusal(name);
		if (refusal) return refusal;
		this.current = name.trim();
		try {
			this.storage?.setItem(KEY, this.current);
		} catch {}
		for (const listener of this.listeners) listener();
		return null;
	}

	forget(): void {
		this.current = null;
		try {
			this.storage?.removeItem(KEY);
		} catch {}
		for (const listener of this.listeners) listener();
	}

	subscribe = (listener: () => void): (() => void) => {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	};

	snapshot = (): string | null => this.current;
}

function browserStorage(): Storage | null {
	try {
		return typeof localStorage === "undefined" ? null : localStorage;
	} catch {
		return null;
	}
}

function stored(storage: Storage | null): string | null {
	try {
		const remembered = storage?.getItem(KEY)?.trim();
		return remembered ? remembered : null;
	} catch {
		return null;
	}
}

export const participant = new Participant();

export function useParticipant(): string | null {
	return useSyncExternalStore(participant.subscribe, participant.snapshot, participant.snapshot);
}
