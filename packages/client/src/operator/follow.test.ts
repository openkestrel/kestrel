import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { followChanges } from "./follow";
import { queueKey } from "./queries";
import { transport } from "./transport";

const encoder = new TextEncoder();

function events(...chunks: string[]) {
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

describe("an Organization's change notices", () => {
	it("refetch the Organization on every open, and only what changed on a notice", async () => {
		const client = new QueryClient();
		const invalidated = vi.spyOn(client, "invalidateQueries");
		let streams = 0;
		const operations = transport(async () => {
			streams += 1;
			return streams === 1
				? events("event: open\ndata: {}\n\n", 'event: change\ndata: {"resource":"queue"}\n\n')
				: events("event: open\ndata: {}\n\n");
		});
		const controller = new AbortController();

		void followChanges(operations, client, "acme", controller.signal);
		await vi.waitFor(() => expect(streams).toBe(2));
		await vi.waitFor(() => expect(invalidated).toHaveBeenCalledTimes(3));
		controller.abort();

		const keys = invalidated.mock.calls.map(([filters]) => filters?.queryKey);
		expect(keys).toEqual([["organizations"], queueKey("acme"), ["organizations"]]);
	});
});
