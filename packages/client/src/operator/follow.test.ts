import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { followChanges } from "./follow";
import { queueKey, sessionsKey, workspaceSessionsQuery, workspacesQuery } from "./queries";
import { reservingControlPlane } from "./stream-fake";
import { TabStream } from "./tab-stream";

describe("an Organization's change notices", () => {
	it("refetch the Organization on every open, and only the named Workspace's reads on a notice", async () => {
		const client = new QueryClient();
		const invalidated = vi.spyOn(client, "invalidateQueries");
		const server = reservingControlPlane();
		const stream = new TabStream(server.operations);

		followChanges(stream, client, "acme");
		await vi.waitFor(() => expect(server.opens).toHaveLength(1));
		await vi.waitFor(() => expect(server.puts).toHaveLength(1));
		const id = server.puts[0]?.id ?? "";
		server.send(id, "open", {});
		server.send(id, "change", { resource: "queue" });
		server.send(id, "change", { resource: "session", id: "s1", workspace: "brave-otter" });
		await vi.waitFor(() => expect(invalidated).toHaveBeenCalledTimes(6));
		server.drop();
		await vi.waitFor(() => expect(server.puts).toHaveLength(2));
		server.send(id, "open", {});
		await vi.waitFor(() => expect(invalidated).toHaveBeenCalledTimes(7));
		stream.close();

		expect(server.puts.map((put) => put.body)).toEqual([
			{ kind: "notices", organization: "acme" },
			{ kind: "notices", organization: "acme" },
		]);
		const keys = invalidated.mock.calls.map(([filters]) => filters?.queryKey);
		expect(keys).toEqual([
			["organizations"],
			workspacesQuery("acme").queryKey,
			queueKey("acme"),
			workspacesQuery("acme").queryKey,
			[...sessionsKey("acme"), "s1"],
			workspaceSessionsQuery("acme", "brave-otter").queryKey,
			["organizations"],
		]);
	});
});
