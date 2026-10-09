import type { Action, Diagnostic, Refusal } from "./generated";

export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

type Write = "POST" | "PUT" | "DELETE";

export type StreamEvent = { event: string; id: string | undefined; data: string };

export class Refused extends Error {
	readonly status: number;
	readonly diagnostic: Diagnostic;
	readonly retryAfter: number | undefined;

	constructor(status: number, diagnostic: Diagnostic, retryAfter?: number) {
		super(diagnostic.message);
		this.name = "Refused";
		this.status = status;
		this.diagnostic = diagnostic;
		this.retryAfter = retryAfter;
	}
}

export class Unreachable extends Error {
	readonly diagnostic: Diagnostic;

	constructor(url: string, call: Call, cause: unknown, compose = false) {
		super("the control plane could not be reached", { cause });
		this.name = "Unreachable";
		this.diagnostic = {
			kind: "connection_failed",
			message: this.message,
			field: null,
			context: { url, operation: call.operation },
			next_steps: genericSteps(call, undefined, undefined, compose),
		};
	}
}

type Call = { operation: string; read: boolean };

export function diagnosisOf(error: unknown): Diagnostic {
	if (error instanceof Refused || error instanceof Unreachable) return error.diagnostic;
	const evidence = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
	return {
		kind: "client_failure",
		message: "The browser Client failed",
		field: null,
		context: { operation: "browser", evidence: evidence.slice(0, 200) },
		next_steps: [
			{ action: "inspect_operation", operation: "browser", resource: null, uncertain: false },
		],
	};
}

export function saysCompose(diagnostic: Diagnostic): boolean {
	return diagnostic.next_steps.some((step) => step.action === "check_connection" && step.compose);
}

export function operatorPath(...segments: string[]): string {
	return `/operator/${segments.map(encodeURIComponent).join("/")}`;
}

// The page's own origin is the control plane's (ADR-0043).
export function transport(
	fetch: Fetch = (url, init) => globalThis.fetch(url, init),
	origin?: string,
) {
	async function answered(path: string, init: RequestInit): Promise<Response> {
		const method = init.method ?? "GET";
		const call = { operation: `${method} ${path}`, read: method === "GET" };
		let response: Response;
		try {
			response = await fetch(path, init);
		} catch (error) {
			if (error instanceof DOMException && error.name === "AbortError") throw error;
			throw new Unreachable(origin ?? location.origin, call, error);
		}
		if (!response.ok) {
			const refused = await refusal(response, call);
			const { diagnostic } = refused;
			// The web server in front answers this when the control plane did not answer at all.
			if (diagnostic.kind === "connection_failed" && response.status >= 502) {
				throw new Unreachable(origin ?? location.origin, call, refused, saysCompose(diagnostic));
			}
			throw refused;
		}
		return response;
	}

	return {
		async read<T>(path: string, { signal }: { signal?: AbortSignal } = {}): Promise<T> {
			return decoded<T>(await answered(path, { method: "GET", signal }));
		},

		async readText(path: string, { signal }: { signal?: AbortSignal } = {}): Promise<string> {
			return (await answered(path, { method: "GET", signal })).text();
		},

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
			{ signal }: { signal?: AbortSignal } = {},
		): AsyncGenerator<StreamEvent> {
			const headers = new Headers({ accept: "text/event-stream" });
			const response = await answered(path, { method: "GET", headers, signal });
			if (!response.body) return;
			try {
				yield* parsed(response.body);
			} catch (error) {
				if (signal?.aborted) throw error;
				throw new Unreachable(
					origin ?? location.origin,
					{ operation: `GET ${path}`, read: true },
					error,
				);
			}
		},
	};
}

export type Transport = ReturnType<typeof transport>;

async function decoded<T>(response: Response): Promise<T> {
	const text = await response.text();
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the generated OpenAPI types are the contract; the transport does not validate bodies.
	return (text ? JSON.parse(text) : undefined) as T;
}

async function refusal(response: Response, call: Call): Promise<Refused> {
	const retry = Number(response.headers.get("retry-after"));
	const retryAfter = Number.isFinite(retry) && retry > 0 ? retry : undefined;
	let said: unknown;
	try {
		said = await response.json();
	} catch {}
	if (isDiagnostic(said)) return new Refused(response.status, said, retryAfter);
	const plain = isRefusal(said) ? said : undefined;
	return new Refused(
		response.status,
		{
			kind: "unknown_response",
			message: plain?.message ?? `the control plane answered ${response.status}`,
			field: typeof plain?.field === "string" ? plain.field : null,
			context: {
				service: "control_plane",
				operation: call.operation,
				status: response.status,
				evidence: null,
			},
			next_steps: genericSteps(call, response.status, retryAfter),
		},
		retryAfter,
	);
}

// A write whose answer is lost or unexplained may have landed, so it is inspected, never replayed.
function genericSteps(
	call: Call,
	status: number | undefined,
	retryAfter?: number,
	compose = false,
): Action[] {
	const check: Action = { action: "check_connection", service: "control_plane", compose };
	if (call.read) {
		return [
			{
				action: "retry_read",
				operation: call.operation,
				resource: null,
				retry_after_seconds: retryAfter ?? null,
			},
			check,
		];
	}
	return [
		{
			action: "inspect_operation",
			operation: call.operation,
			resource: null,
			uncertain: status === undefined || status >= 500,
		},
		check,
	];
}

function isRefusal(said: unknown): said is Refusal {
	return (
		typeof said === "object" &&
		said !== null &&
		"message" in said &&
		typeof said.message === "string"
	);
}

function isDiagnostic(said: unknown): said is Diagnostic {
	return (
		typeof said === "object" &&
		said !== null &&
		"kind" in said &&
		typeof said.kind === "string" &&
		"message" in said &&
		typeof said.message === "string" &&
		"next_steps" in said &&
		Array.isArray(said.next_steps)
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
