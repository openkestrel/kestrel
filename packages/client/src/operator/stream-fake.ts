import type { StreamSubscription } from "./generated";
import { transport } from "./transport";

const encoder = new TextEncoder();

export type Put = { token: string; id: string; body: StreamSubscription };

export function json(status: number, body: unknown) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json" },
	});
}

// A control plane that keeps reservations as the operator boundary does, with each connection
// held open until the test drops it.
export function reservingControlPlane(
	answer: (url: string, init: RequestInit) => Response | undefined = () => undefined,
) {
	let reserved = 0;
	const connections: { token: string; body: ReadableStreamDefaultController<Uint8Array> }[] = [];
	const puts: Put[] = [];
	const deletes: { token: string; id: string }[] = [];
	const opens: string[] = [];
	let refuse: ((put: Put) => Response | Promise<Response> | undefined) | undefined;

	const fetch = async (url: string, init: RequestInit = {}) => {
		const path = url.split("/").slice(2);
		if (url === "/operator/streams" && init.method === "PUT") {
			reserved += 1;
			return json(201, { token: `token-${reserved}` });
		}
		const [, token = "", , id = ""] = path;
		if (path.length === 2 && init.method === "GET") {
			opens.push(token);
			const body = new ReadableStream<Uint8Array>({
				start(controller) {
					connections.push({ token, body: controller });
				},
			});
			init.signal?.addEventListener("abort", () => {
				try {
					connections.find((held) => held.token === token)?.body.error(new Error("aborted"));
				} catch {}
			});
			return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
		}
		if (init.method === "PUT") {
			const text = typeof init.body === "string" ? init.body : "";
			// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the TabStream sends the generated subscription.
			const put = { token, id, body: JSON.parse(text) as StreamSubscription };
			puts.push(put);
			return (await refuse?.(put)) ?? new Response(null, { status: 204 });
		}
		if (init.method === "DELETE") {
			deletes.push({ token, id });
			return new Response(null, { status: 204 });
		}
		return answer(url, init) ?? json(404, { message: "unrouted" });
	};

	return {
		operations: transport(fetch),
		puts,
		deletes,
		opens,
		reservations: () => reserved,
		refusing(refusal: (put: Put) => Response | Promise<Response> | undefined) {
			refuse = refusal;
		},
		send(id: string, event: string, data: unknown, cursor?: string) {
			const connection = connections.at(-1);
			if (!connection) throw new Error("no connection is open");
			const envelope = { subscription: id, ...(cursor ? { cursor } : {}), data };
			connection.body.enqueue(
				encoder.encode(`event: ${event}\ndata: ${JSON.stringify(envelope)}\n\n`),
			);
		},
		drop() {
			connections.at(-1)?.body.close();
		},
	};
}
