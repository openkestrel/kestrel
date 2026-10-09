import { describe, expect, it, vi } from "vitest";
import type { StreamSubscription } from "./generated";
import { json, reservingControlPlane as control } from "./stream-fake";
import { TabStream, type Subscriber } from "./tab-stream";
import type { StreamEvent } from "./transport";

function recording(subscription: () => StreamSubscription) {
	const delivered: StreamEvent[] = [];
	const refused: unknown[] = [];
	const subscriber: Subscriber = {
		subscription,
		deliver: (event) => delivered.push(event),
		refused: (error) => refused.push(error),
	};
	return { subscriber, delivered, refused };
}

describe("a tab's stream", () => {
	it("carries every subscription over one reserved connection, each event to its own subscriber", async () => {
		const server = control();
		const tab = new TabStream(server.operations);
		const notices = recording(() => ({ kind: "notices", organization: "acme" }));
		const followed = recording(() => ({
			kind: "transcript",
			organization: "acme",
			workspace: "brave-otter",
		}));

		tab.subscribe(notices.subscriber);
		tab.subscribe(followed.subscriber);
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		server.send(server.puts[0]?.id ?? "", "open", {});
		server.send(server.puts[1]?.id ?? "", "entry", { seq: 1 }, "w:1");

		await vi.waitFor(() => expect(followed.delivered).toHaveLength(1));
		expect(notices.delivered).toEqual([{ event: "open", id: undefined, data: "{}" }]);
		expect(followed.delivered).toEqual([{ event: "entry", id: "w:1", data: '{"seq":1}' }]);
		expect(server.reservations()).toBe(1);
		expect(server.puts.map((put) => put.token)).toEqual(["token-1", "token-1"]);
		tab.close();
	});

	it("changes subscriptions when a view changes, keeping its connection", async () => {
		const server = control();
		const tab = new TabStream(server.operations);
		const first = recording(() => ({ kind: "transcript", organization: "acme", workspace: "one" }));
		const second = recording(() => ({
			kind: "transcript",
			organization: "acme",
			workspace: "two",
		}));

		const leaving = tab.subscribe(first.subscriber);
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		const left = server.puts[0]?.id ?? "";
		leaving.unsubscribe();
		tab.subscribe(second.subscriber);
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		server.send(left, "entry", { seq: 9 }, "w:9");
		server.send(server.puts[1]?.id ?? "", "entry", { seq: 1 }, "w:1");

		await vi.waitFor(() => expect(second.delivered).toHaveLength(1));
		expect(server.deletes).toEqual([{ token: "token-1", id: left }]);
		expect(first.delivered).toEqual([]);
		expect(server.puts[1]?.body.workspace).toBe("two");
		expect(server.opens).toEqual(["token-1"]);
		expect(server.reservations()).toBe(1);
		tab.close();
	});

	it("reserves again after a drop and re-subscribes each subscription as it now stands", async () => {
		const server = control();
		const tab = new TabStream(server.operations);
		let cursor: string | undefined;
		const followed = recording(() => ({
			kind: "transcript",
			organization: "acme",
			workspace: "brave-otter",
			...(cursor ? { after: cursor } : {}),
		}));
		followed.subscriber.deliver = (event) => {
			cursor = event.id ?? cursor;
		};
		const notices = recording(() => ({ kind: "notices", organization: "acme" }));

		tab.subscribe(followed.subscriber);
		tab.subscribe(notices.subscriber);
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		server.send(server.puts[0]?.id ?? "", "entry", { seq: 4 }, "w:4");
		await vi.waitFor(() => expect(cursor).toBe("w:4"));
		server.drop();

		await vi.waitFor(() => expect(server.puts).toHaveLength(4));
		expect(server.reservations()).toBe(2);
		expect(server.puts.slice(2).map((put) => [put.token, put.body])).toEqual([
			[
				"token-2",
				{ kind: "transcript", organization: "acme", workspace: "brave-otter", after: "w:4" },
			],
			["token-2", { kind: "notices", organization: "acme" }],
		]);
		tab.close();
	});

	it("forgets a subscription that ended, so a reconnect does not ask for it again", async () => {
		const server = control();
		const tab = new TabStream(server.operations);
		const sealed = recording(() => ({ kind: "transcript", organization: "acme", workspace: "w" }));

		tab.subscribe(sealed.subscriber);
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		server.send(server.puts[0]?.id ?? "", "end", { because: "sealed" });
		await vi.waitFor(() => expect(sealed.delivered).toHaveLength(1));
		server.drop();

		await vi.waitFor(() => expect(server.reservations()).toBe(2));
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(server.puts).toHaveLength(1);
		tab.close();
	});

	it("reserves again when its reservation is forgotten, and gives a refused subscription up", async () => {
		const server = control();
		let forgotten = true;
		server.refusing((put) => {
			if (put.body.workspace === "nothing") {
				return json(404, {
					kind: "missing_reference",
					message: "no Workspace is named nothing",
					context: {},
					next_steps: [],
				});
			}
			if (forgotten) {
				forgotten = false;
				return json(404, { message: "no such stream reservation, or it has expired" });
			}
			return undefined;
		});
		const tab = new TabStream(server.operations);
		const kept = recording(() => ({ kind: "notices", organization: "acme" }));
		const missing = recording(() => ({
			kind: "transcript",
			organization: "acme",
			workspace: "nothing",
		}));

		tab.subscribe(kept.subscriber);
		await vi.waitFor(() => expect(server.reservations()).toBe(2));
		await vi.waitFor(() =>
			expect(server.puts.map((put) => put.token)).toEqual(["token-1", "token-2"]),
		);
		tab.subscribe(missing.subscriber);
		await vi.waitFor(() => expect(missing.refused).toHaveLength(1));
		await new Promise((resolve) => setTimeout(resolve, 400));

		expect(server.reservations()).toBe(2);
		expect(server.puts.filter((put) => put.body.workspace === "nothing")).toHaveLength(1);
		tab.close();
	});
});
