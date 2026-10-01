import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { Currency } from "./currency";
import type { LinkChannel, LinkMessage } from "./link";
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

// A stand-in operator: every GET opens an SSE stream the test pushes events into, and every
// POST answers no content.
class Wire {
	readonly seen: { url: string; method: string; after: string | null }[] = [];
	readonly operations: Transport;

	private readonly open: ReadableStreamDefaultController<Uint8Array>[] = [];
	private readonly live = new Set<{ url: string }>();

	constructor() {
		this.operations = transport(async (url, init = {}) => {
			const method = init.method ?? "GET";
			const record = { url, method, after: new Headers(init.headers).get("last-event-id") };
			this.seen.push(record);
			if (method !== "GET") return new Response(null, { status: 204 });
			this.live.add(record);
			init.signal?.addEventListener("abort", () => this.live.delete(record));
			const body = new ReadableStream<Uint8Array>({
				start: (controller) => {
					this.open.push(controller);
					if (url.endsWith("/changes")) {
						controller.enqueue(encoder.encode("event: open\ndata: {}\n\n"));
					}
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
					expect(spy).toHaveBeenCalledWith({
						queryKey: ["organizations", "acme", "workspaces"],
					}),
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

	it("invalidates the Workspace, Session or queue read a change names", async () => {
		const wire = new Wire();
		const client = new QueryClient();
		const invalidated = vi.spyOn(client, "invalidateQueries");
		const tab = new Currency({
			queryClient: client,
			operations: wire.operations,
			channel: null,
			settle: 1,
			now: () => 1,
		});
		tab.watch("acme");

		await vi.waitFor(() => expect(wire.requests("/changes")).toBe(1));

		wire.send({ event: "change", id: undefined, data: '{"resource":"session","id":"x"}' });
		wire.send({ event: "change", id: undefined, data: '{"resource":"queue"}' });

		await vi.waitFor(() =>
			expect(invalidated).toHaveBeenCalledWith({
				queryKey: ["organizations", "acme", "sessions"],
			}),
		);
		await vi.waitFor(() =>
			expect(invalidated).toHaveBeenCalledWith({ queryKey: ["organizations", "acme", "queue"] }),
		);

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
});
