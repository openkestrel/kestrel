import { describe, expect, it } from "vitest";
import { ATTEMPTS, outageOf, untilAttempt } from "./outage";
import { Refused, transport } from "./transport";

const ORIGIN = "http://localhost:7719";

async function failureOf(respond: () => Response | Promise<Response>): Promise<unknown> {
	try {
		await transport(async () => respond(), ORIGIN).read("/operator/organizations");
	} catch (error) {
		return error;
	}
	throw new Error("the read succeeded");
}

function json(status: number, body: unknown) {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "content-type": "application/json" },
	});
}

const proxied = (compose: boolean) =>
	json(502, {
		kind: "connection_failed",
		message: "kestrel isn't running",
		field: null,
		context: { url: ORIGIN, operation: "GET /operator/organizations" },
		next_steps: [{ action: "check_connection", service: "control_plane", compose }],
	});

describe("an outage", () => {
	it("is a control plane the server in front could not reach, with its compose evidence", async () => {
		expect(outageOf(await failureOf(() => proxied(true)))).toEqual({
			origin: ORIGIN,
			compose: true,
		});
		expect(outageOf(await failureOf(() => proxied(false)))).toEqual({
			origin: ORIGIN,
			compose: false,
		});
	});

	it("is a control plane nothing answered for", async () => {
		const failed = await failureOf(() => {
			throw new TypeError("Failed to fetch");
		});

		expect(outageOf(failed)).toEqual({ origin: ORIGIN, compose: false });
	});

	it("is not a refusal, a busy control plane or an answer it could not read", async () => {
		const refused = await failureOf(() =>
			json(404, {
				kind: "missing_reference",
				message: "no Organization acme",
				field: null,
				context: {},
				next_steps: [],
			}),
		);
		const busy = await failureOf(() =>
			json(503, {
				kind: "unavailable",
				message: "the control plane could not answer",
				field: null,
				context: {},
				next_steps: [],
			}),
		);
		const garbled = await failureOf(() => new Response("upstream gone", { status: 502 }));

		for (const failure of [refused, busy, garbled]) {
			expect(failure).toBeInstanceOf(Refused);
			expect(outageOf(failure)).toBeUndefined();
		}
		expect(outageOf(new Error("a fault in the page"))).toBeUndefined();
	});
});

describe("asking again", () => {
	it("waits longer each time, up to half a minute", () => {
		const waits = Array.from(
			{ length: ATTEMPTS },
			(_, attempt) => untilAttempt(attempt) ?? Number.NaN,
		);

		expect(waits.slice(0, 4)).toEqual([2_000, 4_000, 8_000, 16_000]);
		expect(Math.max(...waits)).toBe(30_000);
	});

	it("stops after a bounded number of attempts", () => {
		expect(untilAttempt(ATTEMPTS)).toBeUndefined();
	});
});
