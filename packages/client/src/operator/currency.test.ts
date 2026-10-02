import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { Currency } from "./currency";
import type { LinkChannel, LinkMessage } from "./link";
import { Participant } from "./participant";
import { transport, type StreamEvent, type Transport } from "./transport";

const encoder = new TextEncoder();

class Bus {
	private readonly listeners = new Set<(message: LinkMessage) => void>();

	post(message: LinkMessage): void {
		for (const listener of this.listeners) listener(message);
	}

	subscribe(listener: (message: LinkMessage) => void): () => void {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}
}

function channelOf(bus: Bus): LinkChannel {
	return {
		post: (message) => queueMicrotask(() => bus.post(message)),
		subscribe: (listener) => bus.subscribe(listener),
		close: () => {},
	};
}

class Wire {
	readonly seen: { url: string; method: string; after: string | null }[] = [];
	readonly operations: Transport;

	private readonly open: ReadableStreamDefaultController<Uint8Array>[] = [];
	private readonly live = new Set<{ url: string }>();

	constructor(
		private readonly greets = true,
		private readonly refuse: string | null = null,
	) {
		this.operations = transport(async (url, init = {}) => {
			const method = init.method ?? "GET";
			const record = { url, method, after: new Headers(init.headers).get("last-event-id") };
			this.seen.push(record);
			if (method !== "GET") return new Response(null, { status: 204 });
			if (this.refuse !== null && url.includes(this.refuse)) {
				return new Response(null, { status: 403 });
			}
			this.live.add(record);
			init.signal?.addEventListener("abort", () => this.live.delete(record));
			const body = new ReadableStream<Uint8Array>({
				start: (controller) => {
					this.open.push(controller);
					if (this.greets && url.endsWith("/changes")) {
						controller.enqueue(encoder.encode("event: open\ndata: {}\n\n"));
					}
					if (url.includes("follow=false")) controller.close();
				},
			});
			return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
		});
	}

	send(event: StreamEvent): void {
		const text = `${event.id ? `id: ${event.id}\n` : ""}event: ${event.event}\ndata: ${event.data}\n\n`;
		for (const controller of this.open) controller.enqueue(encoder.encode(text));
	}

	requests(needle: string): number {
		return this.seen.filter((request) => request.url.includes(needle)).length;
	}

	held(needle: string): number {
		return [...this.live].filter((request) => request.url.includes(needle)).length;
	}
}

function follows(wires: Wire[]): number {
	return wires.reduce((count, wire) => count + wire.requests("follow=true"), 0);
}

function polls(wires: Wire[]): number {
	return wires.reduce((count, wire) => count + wire.requests("follow=false"), 0);
}

function held(wires: Wire[], needle: string): number {
	return wires.reduce((count, wire) => count + wire.held(needle), 0);
}

describe("the four-stream budget", () => {
	it("counts each Organization's notice stream against the follow slots", async () => {
		const bus = new Bus();
		const wires: Wire[] = [];
		const tabs: Currency[] = [];
		let clock = 100;
		for (let index = 0; index < 5; index += 1) {
			const wire = new Wire();
			const tab = new Currency({
				queryClient: new QueryClient(),
				operations: wire.operations,
				channel: channelOf(bus),
				settle: 5,
				heartbeat: 1_000,
				liveness: 1_000,
				poll: 5,
				now: () => clock++,
				visible: () => true,
			});
			const organization = index < 4 ? "acme" : "globex";
			tab.watch(organization);
			tab.watchFollow(organization, `workspace-${index}`);
			wires.push(wire);
			tabs.push(tab);
		}

		await vi.waitFor(
			() => {
				expect(held(wires, "/changes")).toBe(2);
				expect(polls(wires)).toBeGreaterThan(0);
			},
			{ timeout: 2_000 },
		);
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(held(wires, "/changes") + held(wires, "follow=true")).toBe(4);
		expect(follows(wires)).toBe(2);

		for (const tab of tabs) tab.close();
	});

	it("polls an Organization beyond four notice streams instead of opening a fifth", async () => {
		const bus = new Bus();
		const wires: Wire[] = [];
		const tabs: Currency[] = [];
		const clients: QueryClient[] = [];
		let clock = 100;
		for (let index = 0; index < 5; index += 1) {
			const wire = new Wire();
			const client = new QueryClient();
			const tab = new Currency({
				queryClient: client,
				operations: wire.operations,
				channel: channelOf(bus),
				settle: 5,
				heartbeat: 1_000,
				liveness: 1_000,
				poll: 5,
				now: () => clock++,
			});
			tab.watch(`organization-${index}`);
			wires.push(wire);
			tabs.push(tab);
			clients.push(client);
		}

		await vi.waitFor(() => expect(held(wires, "/changes")).toBe(4), { timeout: 2_000 });
		const last = clients[4] ?? new QueryClient();
		last.setQueryData(["organizations", "organization-4", "queue"], {});
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(held(wires, "/changes")).toBe(4);
		expect(wires[4]?.requests("/changes")).toBe(0);
		expect(last.getQueryState(["organizations", "organization-4", "queue"])?.isInvalidated).toBe(
			true,
		);

		for (const tab of tabs) tab.close();
	});
});

describe("Organization notices", () => {
	it("shares one notice stream, relays its changes, and hands over when the leader closes", async () => {
		const bus = new Bus();
		const wires = [new Wire(), new Wire()];
		const clients = [new QueryClient(), new QueryClient()];
		const invalidated = clients.map((client) => vi.spyOn(client, "invalidateQueries"));
		let clock = 100;
		const tabs = wires.map(
			(wire, index) =>
				new Currency({
					queryClient: clients[index] ?? new QueryClient(),
					operations: wire.operations,
					channel: channelOf(bus),
					settle: 5,
					heartbeat: 1_000,
					liveness: 1_000,
					now: () => clock++,
				}),
		);

		tabs[0]?.watch("acme");
		tabs[1]?.watch("acme");

		await vi.waitFor(() => expect(wires[0]?.held("/changes")).toBe(1));
		await vi.waitFor(() => expect(wires[1]?.held("/changes")).toBe(0));
		await vi.waitFor(() => expect(invalidated[0]).toHaveBeenCalled());

		const change: StreamEvent = {
			event: "change",
			id: undefined,
			data: '{"resource":"workspace","id":"00000000-0000-0000-0000-000000000001"}',
		};
		wires[0]?.send(change);

		await Promise.all(
			invalidated.map((spy) =>
				vi.waitFor(() =>
					expect(spy).toHaveBeenCalledWith(
						expect.objectContaining({ queryKey: ["organizations", "acme", "workspaces"] }),
					),
				),
			),
		);

		invalidated[1]?.mockClear();
		bus.post({ kind: "refetch", tab: "another-tab", organization: "acme" });
		await vi.waitFor(() => expect(invalidated[1]).toHaveBeenCalled());

		tabs[0]?.close();

		await vi.waitFor(() => expect(wires[1]?.held("/changes")).toBe(1));
		await vi.waitFor(() => expect(invalidated[1]).toHaveBeenCalled());

		tabs[1]?.close();
	});

	it("refetches only the Workspace, Session or queue read a change names", async () => {
		const wire = new Wire();
		const client = new QueryClient();
		const keys = {
			organizations: ["organizations"],
			list: ["organizations", "acme", "workspaces"],
			otter: ["organizations", "acme", "workspaces", "brave-otter"],
			otterWork: ["organizations", "acme", "workspaces", "brave-otter", "work"],
			otterCommits: ["organizations", "acme", "workspaces", "brave-otter", "instance", "commits"],
			heronWork: ["organizations", "acme", "workspaces", "calm-heron", "work"],
			heronCommits: ["organizations", "acme", "workspaces", "calm-heron", "instance", "commits"],
			otterSessions: ["organizations", "acme", "sessions", "workspace", "brave-otter"],
			heronSessions: ["organizations", "acme", "sessions", "workspace", "calm-heron"],
			first: ["organizations", "acme", "sessions", "s1"],
			queue: ["organizations", "acme", "queue"],
			globexQueue: ["organizations", "globex", "queue"],
			range: ["transcript", "acme", "brave-otter", 1, 5],
			payload: ["transcript", "acme", "brave-otter", "payload", "p1"],
		};
		const seed = () => {
			for (const key of Object.values(keys)) client.setQueryData(key, {});
			client.setQueryData(keys.list, [
				{ id: "w1", name: "brave-otter" },
				{ id: "w2", name: "calm-heron" },
			]);
			client.setQueryData(keys.otterSessions, [{ id: "s1" }]);
			client.setQueryData(keys.heronSessions, [{ id: "s2" }]);
		};
		const refetched = () =>
			Object.entries(keys)
				.filter(([, key]) => client.getQueryState(key)?.isInvalidated)
				.map(([name]) => name)
				.toSorted();
		seed();
		const tab = new Currency({
			queryClient: client,
			operations: wire.operations,
			channel: null,
			settle: 1,
			now: () => 1,
		});
		tab.watch("acme");

		await vi.waitFor(() => expect(client.getQueryState(keys.list)?.isInvalidated).toBe(true));
		expect(refetched()).toEqual(
			[
				"organizations",
				"list",
				"otter",
				"otterWork",
				"otterCommits",
				"heronWork",
				"heronCommits",
				"otterSessions",
				"heronSessions",
				"first",
				"queue",
			].toSorted(),
		);

		const changed = async (data: string, expected: string[]) => {
			seed();
			wire.send({ event: "change", id: undefined, data });
			await vi.waitFor(() => expect(refetched()).toEqual(expected.toSorted()));
		};

		await changed('{"resource":"workspace","id":"w1"}', [
			"list",
			"otter",
			"otterWork",
			"otterCommits",
		]);
		await changed('{"resource":"session","id":"s1"}', ["first", "otterSessions"]);
		await changed('{"resource":"session","id":"s9"}', ["otterSessions", "heronSessions"]);
		await changed('{"resource":"queue"}', ["queue"]);

		tab.close();
	});

	it("pages a polled view without refetching what the notices keep current", async () => {
		const wire = new Wire(true, "follow=true");
		const client = new QueryClient();
		const invalidated = vi.spyOn(client, "invalidateQueries");
		const tab = new Currency({
			queryClient: client,
			operations: wire.operations,
			channel: null,
			settle: 1,
			poll: 5,
			now: () => 1,
			visible: () => true,
		});
		tab.watch("acme");
		tab.watchFollow("acme", "brave-otter");

		await vi.waitFor(() => expect(wire.requests("follow=false")).toBeGreaterThan(3));
		invalidated.mockClear();
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(wire.requests("follow=false")).toBeGreaterThan(5);
		expect(invalidated).not.toHaveBeenCalled();

		tab.close();
	});

	it("backs off a refused notice stream instead of reopening it every heartbeat", async () => {
		const wire = new Wire(true, "/changes");
		const tab = new Currency({
			queryClient: new QueryClient(),
			operations: wire.operations,
			channel: channelOf(new Bus()),
			settle: 1,
			heartbeat: 10,
			now: () => 1,
		});
		tab.watch("acme");

		await vi.waitFor(() => expect(wire.requests("/changes")).toBeGreaterThan(0));
		await new Promise((resolve) => setTimeout(resolve, 200));
		expect(wire.requests("/changes")).toBeLessThanOrEqual(2);

		tab.close();
	});

	it("releases a read on a peer's watching claim only once its open handshake arrives", async () => {
		const bus = new Bus();
		const tab = new Currency({
			queryClient: new QueryClient(),
			operations: new Wire().operations,
			channel: channelOf(bus),
			settle: 10_000,
			now: () => 1,
		});

		let released = false;
		void tab.ready("acme").then(() => {
			released = true;
		});

		bus.post({
			kind: "alive",
			tab: "peer",
			watching: { organization: "acme", at: 0 },
			wish: null,
			open: null,
		});
		await new Promise((resolve) => setTimeout(resolve, 20));
		expect(released).toBe(false);

		bus.post({
			kind: "alive",
			tab: "peer",
			watching: null,
			wish: null,
			open: "acme",
		});
		await new Promise((resolve) => setTimeout(resolve, 20));
		expect(released).toBe(false);

		bus.post({
			kind: "alive",
			tab: "peer",
			watching: { organization: "acme", at: 0 },
			wish: null,
			open: "acme",
		});
		await vi.waitFor(() => expect(released).toBe(true));

		tab.close();
	});
});

describe("follow slots", () => {
	it("follows with three visible views, polls with the rest, and gives a hidden tab nothing", async () => {
		const bus = new Bus();
		const wires: Wire[] = [];
		const tabs: Currency[] = [];
		let clock = 100;
		for (let index = 0; index < 6; index += 1) {
			const wire = new Wire();
			const tab = new Currency({
				queryClient: new QueryClient(),
				operations: wire.operations,
				channel: channelOf(bus),
				settle: 5,
				heartbeat: 1_000,
				liveness: 1_000,
				poll: 5,
				now: () => clock++,
				visible: () => index < 5,
			});
			wires.push(wire);
			tabs.push(tab);
			tab.watch("acme");
			tab.watchFollow("acme", `workspace-${index}`);
		}

		await vi.waitFor(
			() => {
				expect(follows(wires)).toBe(3);
				expect(polls(wires)).toBeGreaterThan(0);
			},
			{ timeout: 2_000 },
		);
		expect(wires[5]?.seen).toHaveLength(0);

		tabs[0]?.close();

		await vi.waitFor(() => expect(wires[3]?.requests("follow=true")).toBeGreaterThan(0), {
			timeout: 2_000,
		});

		for (const tab of tabs) tab.close();
	});

	it("follows again on visibility return and refetches current state", async () => {
		const client = new QueryClient();
		const invalidated = vi.spyOn(client, "invalidateQueries");
		const wire = new Wire();
		let shown = false;
		const tab = new Currency({
			queryClient: client,
			operations: wire.operations,
			channel: null,
			settle: 1,
			poll: 5,
			now: () => 7,
			visible: () => shown,
		});
		tab.watchFollow("acme", "brave-otter");

		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(wire.requests("/transcript")).toBe(0);

		shown = true;
		tab.visibilityChanged();

		await vi.waitFor(() => expect(wire.requests("follow=true")).toBe(1));
		expect(invalidated).toHaveBeenCalledWith({
			queryKey: ["organizations", "acme", "workspaces", "brave-otter"],
		});

		tab.close();
	});
	it("follows anonymously until a name is declared, then follows again under it", async () => {
		const wire = new Wire();
		const remembered = new Map<string, string>();
		const person = new Participant({
			getItem: (key) => remembered.get(key) ?? null,
			setItem: (key, value) => remembered.set(key, value),
			removeItem: (key) => remembered.delete(key),
		});
		const tab = new Currency({
			queryClient: new QueryClient(),
			operations: wire.operations,
			channel: null,
			participant: person,
			settle: 1,
			poll: 5,
			now: () => 7,
			visible: () => true,
		});
		tab.watchFollow("acme", "brave-otter");

		await vi.waitFor(() => expect(wire.requests("follow=true")).toBe(1));
		expect(wire.requests("as=")).toBe(0);

		person.remember("jill");

		await vi.waitFor(() => expect(wire.held("follow=true&as=jill")).toBe(1));
		expect(wire.held("follow=true")).toBe(1);

		tab.close();
	});
});
