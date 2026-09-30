import type { Refusal } from "./generated";

export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

type Write = "POST" | "PUT" | "DELETE";

export type StreamEvent = { event: string; id: string | undefined; data: string };

export class Refused extends Error {
	readonly status: number;
	readonly field: string | undefined;
	readonly phase: string | undefined;
	readonly retryAfter: number | undefined;

	constructor(
		status: number,
		message: string,
		{ field, phase, retryAfter }: { field?: string; phase?: string; retryAfter?: number } = {},
	) {
		super(message);
		this.name = "Refused";
		this.status = status;
		this.field = field;
		this.phase = phase;
		this.retryAfter = retryAfter;
	}
}

export class Unreachable extends Error {
	constructor(cause: unknown) {
		super("the control plane could not be reached", { cause });
		this.name = "Unreachable";
	}
}

export function operatorPath(...segments: string[]): string {
	return `/operator/${segments.map(encodeURIComponent).join("/")}`;
}

export function transport(fetch: Fetch = (url, init) => globalThis.fetch(url, init)) {
	async function answered(path: string, init: RequestInit): Promise<Response> {
		let response: Response;
		try {
			response = await fetch(path, init);
		} catch (error) {
			if (error instanceof DOMException && error.name === "AbortError") throw error;
			throw new Unreachable(error);
		}
		if (!response.ok) throw await refusal(response);
		return response;
	}

	async function body<T>(response: Response): Promise<T> {
		const text = await response.text();
		return (text ? JSON.parse(text) : undefined) as T;
	}

	return {
		async read<T>(path: string, { signal }: { signal?: AbortSignal } = {}): Promise<T> {
			return body<T>(await answered(path, { method: "GET", signal }));
		},

		// The header is what a cross-origin form cannot send (ADR-0036).
		async write<T>(
			method: Write,
			path: string,
			payload?: unknown,
			{ signal }: { signal?: AbortSignal } = {},
		): Promise<T> {
			const headers = new Headers({ "X-Kestrel-Operator": "1" });
			if (payload !== undefined) headers.set("content-type", "application/json");
			return body<T>(
				await answered(path, {
					method,
					headers,
					body: payload === undefined ? undefined : JSON.stringify(payload),
					signal,
				}),
			);
		},

		async *stream(
			path: string,
			{ after, signal }: { after?: string; signal?: AbortSignal } = {},
		): AsyncGenerator<StreamEvent> {
			const headers = new Headers({ accept: "text/event-stream" });
			if (after !== undefined) headers.set("Last-Event-ID", after);
			const response = await answered(path, { method: "GET", headers, signal });
			if (!response.body) return;
			yield* parsed(response.body);
		},
	};
}

export type Transport = ReturnType<typeof transport>;

async function refusal(response: Response): Promise<Refused> {
	const retry = Number(response.headers.get("retry-after"));
	const retryAfter = Number.isFinite(retry) && retry > 0 ? retry : undefined;
	let said: Partial<Refusal & { field: unknown; phase: unknown }> = {};
	try {
		said = await response.json();
	} catch {}
	return new Refused(
		response.status,
		typeof said.message === "string"
			? said.message
			: `the control plane answered ${response.status}`,
		{
			field: typeof said.field === "string" ? said.field : undefined,
			phase: typeof said.phase === "string" ? said.phase : undefined,
			retryAfter,
		},
	);
}

async function* parsed(body: ReadableStream<Uint8Array>): AsyncGenerator<StreamEvent> {
	const decoder = new TextDecoder();
	let buffered = "";
	let event = "";
	let id: string | undefined;
	let data: string[] = [];

	for await (const chunk of body as unknown as AsyncIterable<Uint8Array>) {
		buffered += decoder.decode(chunk, { stream: true });
		let end = buffered.search(/\r\n|\r|\n/);
		while (end !== -1) {
			const line = buffered.slice(0, end);
			const breakLength = buffered.startsWith("\r\n", end) ? 2 : 1;
			// A lone \r at a chunk's end may be the first half of \r\n.
			if (buffered[end] === "\r" && end + 1 === buffered.length) break;
			buffered = buffered.slice(end + breakLength);

			if (line === "") {
				if (data.length > 0) yield { event: event || "message", id, data: data.join("\n") };
				event = "";
				id = undefined;
				data = [];
			} else if (!line.startsWith(":")) {
				const colon = line.indexOf(":");
				const field = colon === -1 ? line : line.slice(0, colon);
				const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
				if (field === "event") event = value;
				else if (field === "id") id = value;
				else if (field === "data") data.push(value);
			}
			end = buffered.search(/\r\n|\r|\n/);
		}
	}
}
