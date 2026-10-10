import type { StreamEvent as Envelope, StreamReservation, StreamSubscription } from "./generated";
import { operatorPath, Refused, type StreamEvent, type Transport } from "./transport";

const RETRY = 250;
const RETRY_CAP = 5_000;

export type Subscriber = {
	// Asked at every subscribe, so a resubscription resumes from the cursor last delivered.
	subscription(): StreamSubscription;
	deliver(event: StreamEvent): void;
	// The connection dropped, with its failure when it failed rather than closed.
	dropped?(failure: unknown): void;
	// The subscription itself was refused; it waits for a retry.
	refused?(error: unknown): void;
};

export type Subscribed = {
	unsubscribe(): void;
	resubscribe(): void;
	// Subscribes a refused subscription again, or cuts the connection's backoff short.
	retry(): void;
};

// One event stream for the tab's whole life (ADR-0045): views subscribe and unsubscribe over
// requests, and any drop reserves again and re-subscribes everything still wanted.
export class TabStream {
	private readonly subscribers = new Map<string, Subscriber>();
	private readonly refusals = new Map<string, Subscriber>();
	private failure: unknown;
	private wake: (() => void) | undefined;
	private readonly putting = new Map<string, Promise<void>>();
	private named = 0;
	private token: string | undefined;
	private connection: AbortController | undefined;
	private running = false;
	private closed = false;

	constructor(private readonly operations: Transport) {}

	subscribe(subscriber: Subscriber): Subscribed {
		this.named += 1;
		const id = `s${this.named}`;
		this.subscribers.set(id, subscriber);
		if (this.token !== undefined) void this.put(this.token, id, subscriber);
		this.run();
		return {
			unsubscribe: () => this.unsubscribe(id),
			resubscribe: () => {
				if (this.token !== undefined && this.subscribers.get(id) === subscriber) {
					void this.put(this.token, id, subscriber);
				}
			},
			retry: () => {
				if (this.refusals.get(id) === subscriber) {
					this.refusals.delete(id);
					this.subscribers.set(id, subscriber);
					if (this.token !== undefined) void this.put(this.token, id, subscriber);
				}
				this.wake?.();
			},
		};
	}

	close(): void {
		this.closed = true;
		this.connection?.abort();
		this.wake?.();
	}

	private unsubscribe(id: string): void {
		this.refusals.delete(id);
		if (!this.subscribers.delete(id) || this.token === undefined) return;
		const path = operatorPath("streams", this.token, "subscriptions", id);
		// A DELETE that overtook its PUT would leave the subscription, and its follower, unowned.
		void (this.putting.get(id) ?? Promise.resolve())
			.then(() => this.operations.write("DELETE", path))
			.catch(() => {});
	}

	private run(): void {
		if (this.running) return;
		this.running = true;
		void this.loop();
	}

	private async loop(): Promise<void> {
		let backoff = RETRY;
		while (!this.closed) {
			const connection = new AbortController();
			this.connection = connection;
			this.failure = undefined;
			let failure: unknown;
			try {
				// oxlint-disable-next-line no-await-in-loop -- each connection is reserved after the last one dropped.
				const { token } = await this.operations.write<StreamReservation>(
					"PUT",
					operatorPath("streams"),
					undefined,
					{ signal: connection.signal },
				);
				this.token = token;
				for (const [id, subscriber] of this.subscribers) void this.put(token, id, subscriber);
				// oxlint-disable-next-line no-await-in-loop -- the next connection starts after this one ends.
				for await (const event of this.operations.stream(operatorPath("streams", token), {
					signal: connection.signal,
				})) {
					backoff = RETRY;
					this.dispatch(event);
				}
			} catch (error) {
				failure = this.failure ?? error;
			}
			this.token = undefined;
			connection.abort();
			// oxlint-disable-next-line typescript/no-unnecessary-condition -- close() can run while the stream awaits.
			if (this.closed) return;
			for (const subscriber of this.subscribers.values()) subscriber.dropped?.(failure);
			// oxlint-disable-next-line no-await-in-loop -- the backoff must grow between attempts.
			await this.sleep(backoff);
			backoff = Math.min(backoff * 2, RETRY_CAP);
		}
	}

	private sleep(milliseconds: number): Promise<void> {
		return new Promise((resolve) => {
			const timer = setTimeout(resolve, milliseconds);
			this.wake = () => {
				clearTimeout(timer);
				this.wake = undefined;
				resolve();
			};
		});
	}

	private dispatch(event: StreamEvent): void {
		let envelope: Envelope;
		try {
			// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the envelope is the generated OpenAPI type; the transport does not validate bodies.
			envelope = JSON.parse(event.data) as Envelope;
		} catch {
			return;
		}
		const subscriber = this.subscribers.get(envelope.subscription);
		if (!subscriber) return;
		if (event.event === "end") this.subscribers.delete(envelope.subscription);
		subscriber.deliver({
			event: event.event,
			id: envelope.cursor,
			data: JSON.stringify(envelope.data),
		});
	}

	private put(token: string, id: string, subscriber: Subscriber): Promise<void> {
		const put = this.subscribing(token, id, subscriber).finally(() => {
			if (this.putting.get(id) === put) this.putting.delete(id);
		});
		this.putting.set(id, put);
		return put;
	}

	// A plain 404 is the reservation forgotten; a typed refusal is the subscription's own.
	private async subscribing(token: string, id: string, subscriber: Subscriber): Promise<void> {
		try {
			await this.operations.write(
				"PUT",
				operatorPath("streams", token, "subscriptions", id),
				subscriber.subscription(),
			);
		} catch (error) {
			if (this.token !== token || this.subscribers.get(id) !== subscriber) return;
			if (
				error instanceof Refused &&
				error.diagnostic.kind !== "unknown_response" &&
				error.status < 500
			) {
				this.subscribers.delete(id);
				this.refusals.set(id, subscriber);
				subscriber.refused?.(error);
				return;
			}
			// The connection is cut for this answer, which names the failure better than the abort.
			this.failure = error;
			this.connection?.abort();
		}
	}
}
