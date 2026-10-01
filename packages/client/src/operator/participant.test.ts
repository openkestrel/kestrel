import { describe, expect, it, vi } from "vitest";
import { nameRefusal, Participant } from "./participant";

type Stored = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function storage(initial: Record<string, string> = {}): Stored {
	const held = new Map(Object.entries(initial));
	return {
		getItem: (key) => held.get(key) ?? null,
		setItem: (key, value) => {
			held.set(key, value);
		},
		removeItem: (key) => {
			held.delete(key);
		},
	};
}

describe("a declared name", () => {
	it("is remembered per browser and read back by the next visit", () => {
		const held = storage();
		new Participant(held).remember("jack");

		expect(new Participant(held).name()).toBe("jack");
	});

	it("is trimmed, and refused when it is empty, over-long or carries a control character", () => {
		expect(new Participant(storage()).remember("  jack  ")).toBeNull();
		expect(nameRefusal("")).toContain("A name is needed");
		expect(nameRefusal("x".repeat(65))).toContain("at most 64 characters");
		expect(nameRefusal("jack\u0000")).toContain("control character");
	});

	it("is forgotten when a person changes it", () => {
		const held = storage();
		const person = new Participant(held);
		person.remember("jack");
		person.forget();

		expect(person.name()).toBeNull();
		expect(new Participant(held).name()).toBeNull();
	});

	it("tells every listener when it changes", () => {
		const person = new Participant(storage());
		const heard = vi.fn<() => void>();
		person.subscribe(heard);
		person.remember("jack");
		person.forget();

		expect(heard).toHaveBeenCalledTimes(2);
	});

	it("still writes this visit when the browser refuses storage", () => {
		const person = new Participant(null);
		expect(person.remember("jack")).toBeNull();
		expect(person.name()).toBe("jack");
	});
});
