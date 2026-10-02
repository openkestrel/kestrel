import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { Change } from "./generated";
import { useParticipant } from "./participant";
import { operator, refetchNoticed, refetchOrganization } from "./queries";
import { FollowSession, TranscriptMirror, type TranscriptSnapshot } from "./transcript";
import { operatorPath, type Transport } from "./transport";

const RETRY = 250;
const RETRY_CAP = 5_000;

export function useChangeNotices(organization: string): void {
	const queryClient = useQueryClient();
	useEffect(() => {
		const controller = new AbortController();
		void followChanges(operator, queryClient, organization, controller.signal);
		return () => controller.abort();
	}, [queryClient, organization]);
}

// Every open refetches the whole Organization, so reads that raced the stream's opening are healed.
export async function followChanges(
	operations: Transport,
	client: QueryClient,
	organization: string,
	signal: AbortSignal,
): Promise<void> {
	const path = operatorPath("organizations", organization, "changes");
	let backoff = RETRY;
	while (!signal.aborted) {
		try {
			// oxlint-disable-next-line no-await-in-loop -- the next stream starts after this one ends.
			for await (const event of operations.stream(path, { signal })) {
				backoff = RETRY;
				if (event.event === "change") {
					const change = changeOf(event.data);
					if (change) void refetchNoticed(client, organization, change);
				} else if (event.event === "open" || event.event === "resync") {
					void refetchOrganization(client, organization);
				}
			}
		} catch {}
		if (signal.aborted) return;
		// oxlint-disable-next-line no-await-in-loop -- the backoff must grow between attempts.
		await new Promise((resolve) => setTimeout(resolve, backoff));
		backoff = Math.min(backoff * 2, RETRY_CAP);
	}
}

export function useTranscript(organization: string, workspace: string): TranscriptSnapshot {
	const participant = useParticipant();
	const followed = `${organization}/${workspace}`;
	const [held, hold] = useState(() => ({ followed, mirror: new TranscriptMirror() }));
	let mirror = held.mirror;
	if (held.followed !== followed) {
		mirror = new TranscriptMirror();
		hold({ followed, mirror });
	}
	// A follower registers its name when the follow opens, so a new name reopens the follow.
	useEffect(() => {
		const follow = new FollowSession({
			operations: operator,
			organization,
			workspace,
			participant,
			mirror,
		});
		follow.start();
		return () => follow.stop();
	}, [mirror, organization, workspace, participant]);
	return useSyncExternalStore(mirror.subscribe, mirror.snapshot, mirror.snapshot);
}

function changeOf(data: string): Change | undefined {
	try {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- change bodies are the generated OpenAPI type; the transport does not validate them.
		const change = JSON.parse(data) as Partial<Change> & { id?: unknown };
		if (change.resource === "queue") return { resource: "queue" };
		if (
			(change.resource === "workspace" || change.resource === "session") &&
			typeof change.id === "string"
		) {
			return { resource: change.resource, id: change.id };
		}
	} catch {}
	return undefined;
}
