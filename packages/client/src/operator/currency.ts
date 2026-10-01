import type { QueryClient } from "@tanstack/react-query";
import { useEffect, useSyncExternalStore } from "react";
import {
	type LinkChannel,
	type LinkMessage,
	type LinkNotice,
	type Peer,
	sharedChannel,
	type Wish,
	type Watching,
} from "./link";
import { noticedKey, workspaceKey } from "./queries";
import { FollowSession, page, TranscriptMirror, type TranscriptSnapshot } from "./transcript";
import { operatorPath, Refused, type Transport } from "./transport";

// One shared notice stream, and three follow slots beside it: four SSE connections per origin
// at most, leaving two of the six HTTP/1.1 connections for ordinary reads and writes.
const FOLLOW_SLOTS = 3;
const HEARTBEAT = 2_000;
const LIVENESS = 5_000;
const SETTLE = 150;
const NOTICE_RETRY = 250;
const NOTICE_RETRY_CAP = 5_000;
const POLL = 1_000;
const PARTICIPANT = "operator";

type Mode = "follow" | "poll" | "idle";

export type CurrencyOptions = {
	queryClient: QueryClient;
	operations: Transport;
	channel?: LinkChannel | null;
	participant?: string;
	now?: () => number;
	visible?: () => boolean;
	settle?: number;
	heartbeat?: number;
	liveness?: number;
	poll?: number;
};

// Everything this tab shares with the other tabs on its origin: which one holds the
// Organization's notice stream, and which visible views hold a follow slot.
export class Currency {
	readonly queryClient: QueryClient;
	readonly operations: Transport;
	readonly participant: string;

	private readonly channel: LinkChannel | null;
	private readonly now: () => number;
	private readonly shown: () => boolean;
	private readonly settleMillis: number;
	private readonly heartbeatMillis: number;
	private readonly livenessMillis: number;
	private readonly pollMillis: number;
	private readonly tab = crypto.randomUUID();
	private readonly peers = new Map<string, Peer>();
	private readonly controllers = new Map<string, FollowController>();
	private readonly readies = new Map<string, (() => void)[]>();
	private readonly timers: ReturnType<typeof setInterval>[] = [];
	private watching: Watching | null = null;
	private wish: Wish | null = null;
	private noticeOpen: string | null = null;
	private started = false;
	private greeted = false;
	private settling: ReturnType<typeof setTimeout> | undefined;
	private noticeController: AbortController | null = null;
	private noticeOrganization: string | null = null;
	private readonly undo: (() => void)[] = [];
	private unlistened: (() => void) | null = null;

	constructor(options: CurrencyOptions) {
		this.queryClient = options.queryClient;
		this.operations = options.operations;
		this.channel = options.channel === undefined ? sharedChannel() : options.channel;
		this.participant = options.participant ?? PARTICIPANT;
		this.now = options.now ?? (() => Date.now());
		this.shown =
			options.visible ??
			(() => typeof document === "undefined" || document.visibilityState === "visible");
		this.settleMillis = options.settle ?? SETTLE;
		this.heartbeatMillis = options.heartbeat ?? HEARTBEAT;
		this.livenessMillis = options.liveness ?? LIVENESS;
		this.pollMillis = options.poll ?? POLL;
		this.peers.set(this.tab, { watching: null, wish: null, open: null, seen: this.now() });
		this.unlistened = this.channel?.subscribe((message) => this.heard(message)) ?? null;
	}

	// The Organization this tab reads. Callers are route loads; the oldest want leads, so a
	// tab that has been here longest keeps the notice stream.
	watch(organization: string | null): void {
		if (this.watching?.organization === organization) return;
		this.watching = organization === null ? null : { organization, at: this.now() };
		this.start();
		this.announce(true);
	}

	// Route loads await this before reading, so the shared stream is open first.
	ready(organization: string): Promise<void> {
		if (this.readyToRead(organization)) return Promise.resolve();
		this.settle();
		const waiting = this.readies.get(organization) ?? [];
		return new Promise((resolve) => {
			waiting.push(resolve);
			this.readies.set(organization, waiting);
		});
	}

	controllerFor(organization: string, workspace: string): FollowController {
		const key = `${organization}\u0000${workspace}`;
		let controller = this.controllers.get(key);
		if (!controller) {
			controller = new FollowController({
				organization,
				workspace,
				operations: this.operations,
				participant: this.participant,
				mirror: new TranscriptMirror(`kestrel:transcript:${organization}:${workspace}`),
				refetch: () => this.refetchWorkspace(organization, workspace),
				poll: this.pollMillis,
			});
			this.controllers.set(key, controller);
		}
		return controller;
	}

	// Mount a visible view of a Workspace. Hidden or beyond the slot budget, the view polls.
	watchFollow(organization: string, workspace: string): void {
		const controller = this.controllerFor(organization, workspace);
		controller.retain();
		if (this.wish?.organization !== organization || this.wish.workspace !== workspace) {
			this.wish = { organization, workspace, at: this.now(), visible: this.visible() };
			this.start();
			this.announce(true);
		} else {
			this.settle();
		}
	}

	release(organization: string, workspace: string): void {
		const controller = this.controllerFor(organization, workspace);
		controller.release();
		if (
			controller.users === 0 &&
			this.wish?.organization === organization &&
			this.wish.workspace === workspace
		) {
			this.wish = null;
			this.announce();
		}
	}

	close(): void {
		this.broadcast({ kind: "bye", tab: this.tab });
		this.unlistened?.();
		for (const undo of this.undo) undo();
		this.undo.length = 0;
		for (const timer of this.timers) clearInterval(timer);
		clearTimeout(this.settling);
		this.closeNotices();
		for (const controller of this.controllers.values()) controller.setMode("idle");
		this.channel?.close();
		this.started = false;
	}

	private start(): void {
		if (this.started) return;
		this.started = true;
		if (this.channel) {
			this.timers.push(setInterval(() => this.announce(), this.heartbeatMillis));
			this.timers.push(setInterval(() => this.reap(), this.heartbeatMillis));
		}
		if (typeof window !== "undefined") {
			const leaving = () => this.broadcast({ kind: "bye", tab: this.tab });
			window.addEventListener("pagehide", leaving);
			this.undo.push(() => window.removeEventListener("pagehide", leaving));
		}
		if (typeof document !== "undefined") {
			const changed = () => this.visibilityChanged();
			document.addEventListener("visibilitychange", changed);
			this.undo.push(() => document.removeEventListener("visibilitychange", changed));
		}
	}

	visibilityChanged(): void {
		const visible = this.visible();
		if (this.wish && this.wish.visible !== visible) {
			this.wish = { ...this.wish, visible };
			this.announce();
			if (visible) this.refetchWorkspace(this.wish.organization, this.wish.workspace);
		}
		this.settle();
	}

	private visible(): boolean {
		return this.shown();
	}

	private announce(greet = false): void {
		const self = this.peers.get(this.tab);
		if (self) {
			self.watching = this.watching;
			self.wish = this.wish;
			self.seen = this.now();
		}
		this.broadcast({
			kind: greet && !this.greeted ? "hello" : "alive",
			tab: this.tab,
			watching: this.watching,
			wish: this.wish,
			open: this.noticeOpen,
		});
		this.greeted = true;
		this.settle();
	}

	private heard(message: LinkMessage): void {
		switch (message.kind) {
			case "hello":
			case "alive": {
				if (message.tab === this.tab) return;
				const known = this.peers.get(message.tab);
				const moved =
					!known ||
					!sameWatch(known.watching, message.watching) ||
					!sameWish(known.wish, message.wish);
				this.peers.set(message.tab, {
					watching: message.watching,
					wish: message.wish,
					open: message.open,
					seen: this.now(),
				});
				if (message.kind === "hello" && !known) this.announce();
				if (moved) this.settle();
				this.resolveReadies(false);
				break;
			}
			case "bye":
				if (this.peers.delete(message.tab)) this.settle();
				break;
			case "refetch":
				this.refetchAll();
				break;
			case "notice":
				this.applyNotice(message.organization, message.notice);
				break;
		}
	}

	private settle(): void {
		if (this.settling) return;
		this.settling = setTimeout(() => {
			this.settling = undefined;
			this.evaluate();
		}, this.settleMillis);
	}

	private evaluate(): void {
		if (this.watching && this.leader(this.watching.organization) === this.tab) {
			this.openNotices(this.watching.organization);
		} else {
			this.closeNotices();
		}
		for (const [key, controller] of this.controllers) {
			controller.setMode(this.modeOf(key));
		}
		this.resolveReadies(true);
	}

	private readyToRead(organization: string): boolean {
		if (this.noticeOpen === organization) return true;
		for (const [tab, peer] of this.peers) {
			if (
				tab !== this.tab &&
				peer.watching?.organization === organization &&
				peer.open === organization
			) {
				return true;
			}
		}
		return false;
	}

	// A route load waiting to read is released once the election has run, so a refused or
	// unreachable control plane never hangs the view.
	private resolveReadies(attempted: boolean): void {
		for (const [organization, waiting] of this.readies) {
			if (!attempted && !this.readyToRead(organization)) continue;
			this.readies.delete(organization);
			for (const resolve of waiting) resolve();
		}
	}

	private modeOf(key: string): Mode {
		const controller = this.controllers.get(key);
		const wish = this.wish;
		if (
			!controller ||
			!wish ||
			!wish.visible ||
			wish.organization !== controller.organization ||
			wish.workspace !== controller.workspace
		) {
			return "idle";
		}
		if (this.rank() >= FOLLOW_SLOTS) return "poll";
		return controller.refused ? "poll" : "follow";
	}

	private leader(organization: string): string | undefined {
		const candidates: { tab: string; at: number }[] = [];
		for (const [tab, peer] of this.peers) {
			if (peer.watching?.organization === organization) {
				candidates.push({ tab, at: peer.watching.at });
			}
		}
		candidates.sort((one, other) => one.at - other.at || comparison(one.tab, other.tab));
		return candidates[0]?.tab;
	}

	private rank(): number {
		const candidates: { tab: string; at: number }[] = [];
		for (const [tab, peer] of this.peers) {
			if (peer.wish?.visible) candidates.push({ tab, at: peer.wish.at });
		}
		candidates.sort((one, other) => one.at - other.at || comparison(one.tab, other.tab));
		return candidates.findIndex((candidate) => candidate.tab === this.tab);
	}

	private reap(): void {
		const now = this.now();
		let dropped = false;
		for (const [tab, peer] of this.peers) {
			if (tab !== this.tab && now - peer.seen > this.livenessMillis) {
				this.peers.delete(tab);
				dropped = true;
			}
		}
		if (dropped) this.settle();
	}

	private openNotices(organization: string): void {
		if (
			this.noticeOrganization === organization &&
			this.noticeController &&
			!this.noticeController.signal.aborted
		) {
			return;
		}
		this.closeNotices();
		const controller = new AbortController();
		this.noticeController = controller;
		this.noticeOrganization = organization;
		void this.loopNotices(organization, controller);
	}

	private closeNotices(): void {
		this.noticeController?.abort();
		this.noticeController = null;
		this.noticeOrganization = null;
		this.noticeOpen = null;
	}

	private async loopNotices(organization: string, controller: AbortController): Promise<void> {
		let backoff = NOTICE_RETRY;
		while (!controller.signal.aborted) {
			try {
				const path = operatorPath("organizations", organization, "changes");
				// oxlint-disable-next-line no-await-in-loop -- the next stream starts after this one ends.
				for await (const event of this.operations.stream(path, {
					signal: controller.signal,
				})) {
					if (controller.signal.aborted) return;
					backoff = NOTICE_RETRY;
					if (this.noticeController === controller && this.noticeOpen !== organization) {
						this.noticeOpen = organization;
						this.announce();
						this.resolveReadies(false);
					}
					if (event.event === "change") {
						const notice = noticeOf(event.data);
						if (notice) {
							this.broadcast({
								kind: "notice",
								tab: this.tab,
								organization,
								notice,
							});
							this.applyNotice(organization, notice);
						}
					} else if (event.event === "open" || event.event === "resync") {
						this.broadcast({ kind: "refetch", tab: this.tab, organization });
						this.refetchAll();
					}
				}
			} catch (error) {
				if (controller.signal.aborted) return;
				if (error instanceof Refused && error.status < 500) {
					if (this.noticeController === controller) this.closeNotices();
					return;
				}
			}
			if (controller.signal.aborted) return;
			// oxlint-disable-next-line no-await-in-loop -- the backoff must grow between attempts.
			await sleep(backoff);
			backoff = Math.min(backoff * 2, NOTICE_RETRY_CAP);
		}
	}

	private broadcast(message: LinkMessage): void {
		this.channel?.post(message);
	}

	private refetchAll(): void {
		void this.queryClient.invalidateQueries();
	}

	private refetchWorkspace(organization: string, workspace: string): void {
		void this.queryClient.invalidateQueries({ queryKey: workspaceKey(organization, workspace) });
	}

	private applyNotice(organization: string, notice: LinkNotice): void {
		void this.queryClient.invalidateQueries({
			queryKey: noticedKey(organization, notice.resource),
		});
	}
}

export type FollowOptions = {
	organization: string;
	workspace: string;
	operations: Transport;
	participant: string;
	mirror: TranscriptMirror;
	refetch: () => void;
	poll?: number;
};

// One viewed Workspace: the mirror of its Transcript, held while the view is visible and a
// follow slot is held, and paged by polling when it is not.
export class FollowController {
	readonly mirror: TranscriptMirror;
	readonly organization: string;
	readonly workspace: string;
	users = 0;
	refused = false;

	private readonly operations: Transport;
	private readonly participant: string;
	private readonly refetch: () => void;
	private readonly pollMillis: number;
	private mode: Mode = "idle";
	private session: FollowSession | null = null;
	private polling: ReturnType<typeof setInterval> | undefined;
	private pollingController: AbortController | null = null;
	private inFlight = false;

	constructor(options: FollowOptions) {
		this.organization = options.organization;
		this.workspace = options.workspace;
		this.operations = options.operations;
		this.participant = options.participant;
		this.mirror = options.mirror;
		this.refetch = options.refetch;
		this.pollMillis = options.poll ?? POLL;
	}

	retain(): void {
		this.users += 1;
	}

	release(): void {
		this.users = Math.max(0, this.users - 1);
		if (this.users === 0) {
			this.mode = "idle";
			this.apply();
		}
	}

	setMode(mode: Mode): void {
		this.mode = mode;
		this.apply();
	}

	private apply(): void {
		const following = this.mode === "follow" && !this.refused && !this.mirror.sealed;
		const polling = this.mode !== "idle" && !following && !this.mirror.sealed;
		if (following) this.startFollow();
		else this.stopFollow();
		if (polling) this.startPolling();
		else this.stopPolling();
	}

	private startFollow(): void {
		if (this.session) return;
		this.session = new FollowSession({
			operations: this.operations,
			organization: this.organization,
			workspace: this.workspace,
			participant: this.participant,
			mirror: this.mirror,
			onRefused: () => {
				this.refused = true;
				this.apply();
			},
		});
		this.session.start();
	}

	private stopFollow(): void {
		this.session?.stop();
		this.session = null;
	}

	private startPolling(): void {
		if (this.polling) return;
		this.pollingController = new AbortController();
		void this.tick();
		this.polling = setInterval(() => void this.tick(), this.pollMillis);
	}

	private stopPolling(): void {
		clearInterval(this.polling);
		this.polling = undefined;
		this.pollingController?.abort();
		this.pollingController = null;
	}

	private async tick(): Promise<void> {
		if (this.inFlight || this.mirror.sealed) {
			if (this.mirror.sealed) this.stopPolling();
			return;
		}
		this.inFlight = true;
		this.refetch();
		try {
			await page(
				this.operations,
				this.organization,
				this.workspace,
				this.mirror,
				this.pollingController?.signal,
			);
		} catch (error) {
			// A refusal a retry cannot mend ends the polling; anything else is tried on the next tick.
			if (error instanceof Refused && error.status < 500) this.stopPolling();
		} finally {
			this.inFlight = false;
		}
	}
}

export function useTranscript(
	currency: Currency,
	organization: string,
	workspace: string,
): TranscriptSnapshot {
	const controller = currency.controllerFor(organization, workspace);
	useEffect(() => {
		currency.watchFollow(organization, workspace);
		return () => currency.release(organization, workspace);
	}, [currency, organization, workspace]);
	return useSyncExternalStore(controller.mirror.subscribe, controller.mirror.snapshot);
}

function noticeOf(data: string): LinkNotice | undefined {
	try {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- change bodies are the generated OpenAPI type; the transport does not validate them.
		const changed = JSON.parse(data) as { resource?: string; id?: unknown };
		if (changed.resource === "workspace" && typeof changed.id === "string") {
			return { resource: "workspace", id: changed.id };
		}
		if (changed.resource === "session" && typeof changed.id === "string") {
			return { resource: "session", id: changed.id };
		}
		if (changed.resource === "queue") return { resource: "queue" };
	} catch {}
	return undefined;
}

function sameWatch(one: Watching | null, other: Watching | null): boolean {
	return one?.organization === other?.organization && one?.at === other?.at;
}

function sameWish(one: Wish | null, other: Wish | null): boolean {
	return (
		one?.organization === other?.organization &&
		one?.workspace === other?.workspace &&
		one?.at === other?.at &&
		one?.visible === other?.visible
	);
}

function comparison(one: string, other: string): number {
	if (one === other) return 0;
	return one < other ? -1 : 1;
}

function sleep(milliseconds: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
