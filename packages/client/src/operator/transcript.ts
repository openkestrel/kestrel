import type {
	Activity,
	Entry,
	FollowerEvent,
	Presence,
	Recorded,
	TranscriptKind,
	TranscriptSessionState,
} from "./generated";
import { operatorPath, Refused, type StreamEvent, type Transport } from "./transport";

export type Delivered = {
	seq: number;
	kind: TranscriptKind;
	sessionId: string | null;
	appendedAt: string;
	entry: Entry;
};

export type TranscriptSnapshot = {
	entries: Delivered[];
	activities: Activity[];
	sessionState: TranscriptSessionState | undefined;
	cursor: string | undefined;
	presence: Presence | undefined;
	sealed: boolean;
};

const LIMIT = 500;

export function cursorSeq(cursor: string): number | undefined {
	const seq = Number(cursor.slice(cursor.lastIndexOf(":") + 1));
	return Number.isSafeInteger(seq) ? seq : undefined;
}

export class TranscriptMirror {
	private entries: Delivered[] = [];
	private activities: Activity[] = [];
	private sessionStateValue: TranscriptSessionState | undefined;
	private highest = 0;
	private cursorValue: string | undefined;
	private presenceValue: Presence | undefined;
	private sealedValue = false;
	private readonly listeners = new Set<() => void>();
	private cached: TranscriptSnapshot = this.snapshotOf();

	get cursor(): string | undefined {
		return this.cursorValue;
	}

	get sealed(): boolean {
		return this.sealedValue;
	}

	subscribe = (listener: () => void): (() => void) => {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	};

	snapshot = (): TranscriptSnapshot => this.cached;

	apply(event: StreamEvent): void {
		switch (event.event) {
			case "entry": {
				const recorded = parsed<Recorded>(event.data);
				if (recorded) this.entry(recorded, event.id);
				break;
			}
			case "activity": {
				const activity = parsed<Activity>(event.data);
				if (activity) this.summary(activity, event.id);
				break;
			}
			case "session_state": {
				const state = parsed<TranscriptSessionState>(event.data);
				if (state) {
					this.sessionStateValue = state;
					this.changed();
				}
				break;
			}
			case "cursor":
				this.advance(event.id ?? event.data);
				break;
			case "presence": {
				const presence = parsed<Presence>(event.data);
				if (presence) this.present(presence);
				break;
			}
			case "follower":
				break;
			case "end": {
				const end = parsed<{ because?: string }>(event.data);
				if (end?.because === "sealed") this.sealedValue = true;
				this.changed();
				break;
			}
		}
	}

	private entry(recorded: Recorded, cursor: string | undefined): void {
		if (recorded.seq <= this.highest) return;
		this.highest = recorded.seq;
		this.entries.push(delivered(recorded));
		if (this.entries.length > LIMIT) this.entries.splice(0, this.entries.length - LIMIT);
		if (cursor) this.setCursor(cursor);
		else this.changed();
	}

	// A replay must never reopen a closed summary.
	private summary(activity: Activity, cursor: string | undefined): void {
		const known = this.activities.find((existing) => existing.first_seq === activity.first_seq);
		if (known?.closed && !activity.closed) return;
		if (known) {
			this.activities = this.activities.map((existing) =>
				existing.first_seq === activity.first_seq ? activity : existing,
			);
		} else {
			this.activities = [...this.activities, activity].toSorted(
				(one, other) => one.first_seq - other.first_seq,
			);
			if (this.activities.length > LIMIT) {
				this.activities.splice(0, this.activities.length - LIMIT);
			}
		}
		if (cursor) this.setCursor(cursor);
		else this.changed();
	}

	private advance(cursor: string | undefined): void {
		if (cursor) this.setCursor(cursor);
	}

	private setCursor(cursor: string): void {
		this.cursorValue = cursor;
		const seq = cursorSeq(cursor);
		if (seq !== undefined) this.highest = Math.max(this.highest, seq);
		this.changed();
	}

	private present(presence: Presence): void {
		this.presenceValue = presence;
		this.changed();
	}

	private changed(): void {
		this.cached = this.snapshotOf();
		for (const listener of this.listeners) listener();
	}

	private snapshotOf(): TranscriptSnapshot {
		return {
			entries: [...this.entries],
			activities: [...this.activities],
			sessionState: this.sessionStateValue,
			cursor: this.cursorValue,
			presence: this.presenceValue,
			sealed: this.sealedValue,
		};
	}
}

// oxlint-disable-next-line typescript/no-unnecessary-type-parameters -- each call names the OpenAPI body it expects.
function parsed<T>(data: string): T | undefined {
	try {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- SSE bodies are the generated OpenAPI types; the transport does not validate them.
		return JSON.parse(data) as T;
	} catch {
		return undefined;
	}
}

const RETRY = 250;
const RETRY_CAP = 5_000;

export type FollowOptions = {
	operations: Transport;
	organization: string;
	workspace: string;
	participant: string | null;
	mirror: TranscriptMirror;
};

export class FollowSession {
	private controller: AbortController | null = null;
	private renewal: ReturnType<typeof setTimeout> | undefined;
	private stopped = false;

	constructor(private readonly options: FollowOptions) {}

	start(): void {
		void this.loop();
	}

	stop(): void {
		this.stopped = true;
		this.controller?.abort();
		clearTimeout(this.renewal);
	}

	private async loop(): Promise<void> {
		let backoff = RETRY;
		while (!this.stopped) {
			const controller = new AbortController();
			this.controller = controller;
			try {
				// oxlint-disable-next-line no-await-in-loop -- a reconnect waits for this attempt before deciding to make the next.
				await this.consume(controller.signal);
			} catch (error) {
				if (this.stopped) return;
				if (error instanceof Refused && error.status < 500) return;
			}
			if (this.stopped || this.options.mirror.sealed) return;
			// oxlint-disable-next-line no-await-in-loop -- the backoff must grow between attempts.
			await sleep(backoff);
			backoff = Math.min(backoff * 2, RETRY_CAP);
		}
	}

	private async consume(signal: AbortSignal): Promise<void> {
		const { operations, organization, workspace, participant, mirror } = this.options;
		const path = transcriptPath(organization, workspace, {
			follow: true,
			as: participant ?? undefined,
		});
		for await (const event of operations.stream(path, { after: mirror.cursor, signal })) {
			if (event.event === "end") {
				mirror.apply(event);
				return;
			}
			mirror.apply(event);
			if (event.event === "follower") {
				const follower = parsed<FollowerEvent>(event.data);
				if (follower) this.schedule(follower);
			}
		}
	}

	private schedule(follower: FollowerEvent): void {
		clearTimeout(this.renewal);
		this.renewal = setTimeout(() => void this.renew(follower), (follower.lease_seconds * 1000) / 3);
	}

	private async renew(follower: FollowerEvent): Promise<void> {
		if (this.stopped) return;
		try {
			await this.options.operations.write(
				"POST",
				operatorPath(
					"organizations",
					this.options.organization,
					"workspaces",
					this.options.workspace,
					"followers",
					follower.id,
					"lease",
				),
			);
		} catch {
			// A lapsed or unknown follower registers again rather than being revived.
			if (!this.stopped) this.controller?.abort();
			return;
		}
		this.schedule(follower);
	}
}

export type Range = { first: number; last: number };

// These entries never enter the mirror, whose cursor already stands past them.
export async function readRange(
	operations: Transport,
	organization: string,
	workspace: string,
	range: Range,
	signal?: AbortSignal,
): Promise<Delivered[]> {
	const path = transcriptPath(organization, workspace, {
		follow: false,
		kinds: "shared_state,narration,detail",
		first: range.first,
		last: range.last,
	});
	const entries: Delivered[] = [];
	for await (const event of operations.stream(path, { signal })) {
		if (event.event === "entry") {
			const recorded = parsed<Recorded>(event.data);
			if (recorded) entries.push(delivered(recorded));
		}
		if (event.event === "end") break;
	}
	return entries;
}

export type TranscriptQuery = {
	follow: boolean;
	as?: string;
	kinds?: string;
	summaries?: boolean;
	first?: number;
	last?: number;
};

export function transcriptPath(
	organization: string,
	workspace: string,
	query: TranscriptQuery,
): string {
	const parameters = new URLSearchParams({ follow: String(query.follow) });
	if (query.as) parameters.set("as", query.as);
	if (query.kinds) parameters.set("kinds", query.kinds);
	if (query.summaries !== undefined) parameters.set("summaries", String(query.summaries));
	if (query.first !== undefined) parameters.set("first_seq", String(query.first));
	if (query.last !== undefined) parameters.set("last_seq", String(query.last));
	return `${operatorPath("organizations", organization, "workspaces", workspace, "transcript")}?${parameters}`;
}

export function delivered(recorded: Recorded): Delivered {
	return {
		seq: recorded.seq,
		kind: recorded.kind,
		sessionId: recorded.session_id,
		appendedAt: recorded.appended_at,
		entry: recorded.entry,
	};
}

function sleep(milliseconds: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
