import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { Change } from "./generated";
import { useParticipant } from "./participant";
import { operator, refetchNoticed, refetchOrganization } from "./queries";
import { type Subscribed, TabStream } from "./tab-stream";
import { FollowSession, TranscriptMirror, type TranscriptSnapshot } from "./transcript";

const tab = new TabStream(operator);

export function useChangeNotices(organization: string): void {
	const queryClient = useQueryClient();
	useEffect(() => {
		const subscribed = followChanges(tab, queryClient, organization);
		return () => subscribed.unsubscribe();
	}, [queryClient, organization]);
}

// Every subscription opens with `open`, so a resubscription after any drop refetches the whole
// Organization, healing whatever changed unnoticed.
export function followChanges(
	stream: TabStream,
	client: QueryClient,
	organization: string,
): Subscribed {
	return stream.subscribe({
		subscription: () => ({ kind: "notices", organization }),
		deliver: (event) => {
			if (event.event === "change") {
				const change = changeOf(event.data);
				if (change) void refetchNoticed(client, organization, change);
			} else if (event.event === "open" || event.event === "resync") {
				void refetchOrganization(client, organization);
			}
		},
	});
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
	// A follower registers its name when it subscribes, so a new name subscribes again.
	useEffect(() => {
		const follow = new FollowSession({
			stream: tab,
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
		return JSON.parse(data) as Change;
	} catch {
		return undefined;
	}
}
