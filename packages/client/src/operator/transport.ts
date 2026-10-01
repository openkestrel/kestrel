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

	return {
		async read<T>(path: string, { signal }: { signal?: AbortSignal } = {}): Promise<T> {
			return decoded<T>(await answered(path, { method: "GET", signal }));
		},

		async readText(path: string, { signal }: { signal?: AbortSignal } = {}): Promise<string> {
			return (await answered(path, { method: "GET", signal })).text();
		},

		// For answers that may be bytes rather than JSON, such as a Workspace file read.
		async bytes(
			path: string,
			{ signal }: { signal?: AbortSignal } = {},
		): Promise<{ mediaType: string | null; body: ArrayBuffer }> {
			const response = await answered(path, { method: "GET", signal });
			return {
				mediaType: response.headers.get("content-type"),
				body: await response.arrayBuffer(),
			};
		},

		async write<T>(
			method: Write,
			path: string,
			payload?: unknown,
			{ signal }: { signal?: AbortSignal } = {},
		): Promise<T> {
			const init: RequestInit = { method, signal };
			if (payload !== undefined) {
				init.headers = { "content-type": "application/json" };
				init.body = JSON.stringify(payload);
			}
			return decoded<T>(await answered(path, init));
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

async function decoded<T>(response: Response): Promise<T> {
	const text = await response.text();
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the generated OpenAPI types are the contract; the transport does not validate bodies.
	return (text ? JSON.parse(text) : undefined) as T;
}

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

async function* parsed(body: ReadableStream<BufferSource>): AsyncGenerator<StreamEvent> {
	let event = "";
	let id: string | undefined;
	let data: string[] = [];

	for await (const line of lines(body)) {
		if (line === "") {
			if (data.length > 0) yield { event: event || "message", id, data: data.join("\n") };
			event = "";
			id = undefined;
			data = [];
			continue;
		}
		const colon = line.indexOf(":");
		const field = colon === -1 ? line : line.slice(0, colon);
		const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
		if (field === "event") event = value;
		else if (field === "id") id = value;
		else if (field === "data") data.push(value);
	}
}

// A \r ending a chunk stays buffered: it may be the first half of a \r\n.
async function* lines(body: ReadableStream<BufferSource>): AsyncGenerator<string> {
	let rest = "";
	for await (const text of body.pipeThrough(new TextDecoderStream())) {
		const complete = (rest + text).split(/\r\n|\n|\r(?!$)/);
		rest = complete.pop() ?? "";
		yield* complete;
	}
}
