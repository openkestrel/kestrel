import { describe, expect, it } from "vitest";
import { operatorPath, Refused, transport, Unreachable } from "./transport";

type Seen = { url: string; init: RequestInit };

function answering(respond: (seen: Seen) => Response) {
	const seen: Seen[] = [];
	const fetch = async (url: string, init: RequestInit = {}) => {
		const request = { url, init };
		seen.push(request);
		return respond(request);
	};
	return { operator: transport(fetch), seen };
}

function json(status: number, body: unknown, headers: Record<string, string> = {}) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json", ...headers },
	});
}

function events(...chunks: string[]) {
	const encoder = new TextEncoder();
	return new Response(
		new ReadableStream({
			start(controller) {
				for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
				controller.close();
			},
		}),
		{ status: 200, headers: { "content-type": "text/event-stream" } },
	);
}

async function refusalOf(pending: Promise<unknown>): Promise<Refused> {
	try {
		await pending;
	} catch (error) {
		if (error instanceof Refused) return error;
		throw error;
	}
	throw new Error("the request was not refused");
}

describe("a read", () => {
	it("asks the operator interface on this page's own origin", async () => {
		const { operator, seen } = answering(() => json(200, [{ name: "acme" }]));

		expect(await operator.read("/operator/organizations")).toEqual([{ name: "acme" }]);
		expect(seen[0].url).toBe("/operator/organizations");
	});

	it("is refused with the status and the reason the control plane gave", async () => {
		const { operator } = answering(() =>
			json(404, { message: "no Workspace is named brave-otter" }),
		);

		const refused = await refusalOf(
			operator.read("/operator/organizations/acme/workspaces/brave-otter"),
		);

		expect(refused.status).toBe(404);
		expect(refused.message).toBe("no Workspace is named brave-otter");
		expect(refused.kind).toBeUndefined();
		expect(refused.field).toBeUndefined();
		expect(refused.phase).toBeUndefined();
	});

	it("carries the kind of a typed Diagnostic", async () => {
		const { operator } = answering(() =>
			json(404, { kind: "missing_reference", message: "no Workspace is named brave-otter" }),
		);

		const refused = await refusalOf(operator.read("/operator/organizations/acme/workspaces/x"));

		expect(refused.kind).toBe("missing_reference");
	});

	it("carries the field and the phase a refusal names", async () => {
		const { operator } = answering(() =>
			json(409, { message: "the Session is working", field: "value", phase: "working" }),
		);

		const refused = await refusalOf(operator.read("/operator/organizations/acme/queue"));

		expect(refused).toMatchObject({ status: 409, field: "value", phase: "working" });
	});

	it("says when a busy control plane asked to be asked again", async () => {
		const { operator } = answering(() =>
			json(503, { message: "the control plane could not answer" }, { "retry-after": "1" }),
		);

		const refused = await refusalOf(operator.read("/operator/organizations"));

		expect(refused.retryAfter).toBe(1);
	});

	it("is refused plainly when the answer carries no reason", async () => {
		const { operator } = answering(() => new Response("<h1>Bad Gateway</h1>", { status: 502 }));

		const refused = await refusalOf(operator.read("/operator/organizations"));

		expect(refused.status).toBe(502);
		expect(refused.message).toBe("the control plane answered 502");
	});

	it("is unreachable, not refused, when nothing answers", async () => {
		const operator = transport(async () => {
			throw new TypeError("Failed to fetch");
		});

		await expect(operator.read("/operator/organizations")).rejects.toBeInstanceOf(Unreachable);
	});
});

describe("a write", () => {
	it("sends its body as JSON", async () => {
		const { operator, seen } = answering(() => json(201, { name: "acme" }));

		await operator.write("POST", "/operator/organizations", { name: "acme" });

		const headers = new Headers(seen[0].init.headers);
		expect(seen[0].init.method).toBe("POST");
		expect(headers.get("content-type")).toBe("application/json");
		expect(seen[0].init.body).toBe('{"name":"acme"}');
	});

	it("answers nothing for a response with no body", async () => {
		const { operator } = answering(() => new Response(null, { status: 204 }));

		expect(
			await operator.write("DELETE", "/operator/organizations/acme/credentials/X"),
		).toBeUndefined();
	});
});

describe("a stream", () => {
	it("yields each event with its name, id and data, across chunk boundaries", async () => {
		const { operator } = answering(() =>
			events(
				": keep-alive\n\n",
				'event: entry\nid: 7\ndata: {"seq":',
				'7}\n\r\nevent: entry\r\nid: 8\r\ndata: {"seq":8}\r\n\r\n',
				'event: end\ndata: {"because":"caught_up"}\n\n',
			),
		);

		const delivered = [];
		for await (const event of operator.stream(
			"/operator/organizations/acme/workspaces/w/transcript",
		)) {
			delivered.push(event);
		}

		expect(delivered).toEqual([
			{ event: "entry", id: "7", data: '{"seq":7}' },
			{ event: "entry", id: "8", data: '{"seq":8}' },
			{ event: "end", id: undefined, data: '{"because":"caught_up"}' },
		]);
	});

	it("joins the lines of one event's data", async () => {
		const { operator } = answering(() => events("data: one\ndata: two\n\n"));

		const delivered = [];
		for await (const event of operator.stream("/s")) delivered.push(event);

		expect(delivered).toEqual([{ event: "message", id: undefined, data: "one\ntwo" }]);
	});

	it("reads a \\r\\n split across chunks as one line break", async () => {
		const { operator } = answering(() => events("data: one\r", "\ndata: two\r", "\n\r", "\n"));

		const delivered = [];
		for await (const event of operator.stream("/s")) delivered.push(event);

		expect(delivered).toEqual([{ event: "message", id: undefined, data: "one\ntwo" }]);
	});

	it("asks for an event stream", async () => {
		const { operator, seen } = answering(() => events());

		for await (const _ of operator.stream("/s")) {
		}

		expect(new Headers(seen[0].init.headers).get("accept")).toBe("text/event-stream");
	});

	it("is refused before it opens like any other request", async () => {
		const { operator } = answering(() => json(400, { message: "that cursor names no entry" }));

		const refused = await refusalOf(
			(async () => {
				for await (const _ of operator.stream("/s")) {
				}
			})(),
		);

		expect(refused).toMatchObject({ status: 400, message: "that cursor names no entry" });
	});
});

describe("an operator path", () => {
	it("encodes each segment it is given", () => {
		expect(operatorPath("organizations", "acme corp", "workspaces", "a/b")).toBe(
			"/operator/organizations/acme%20corp/workspaces/a%2Fb",
		);
	});
});
