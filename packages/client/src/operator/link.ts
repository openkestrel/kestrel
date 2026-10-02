export type LinkNotice =
	| { resource: "workspace"; id: string }
	| { resource: "session"; id: string }
	| { resource: "queue" };

export type Watching = { organization: string; at: number };

export type Wish = { organization: string; workspace: string; at: number; visible: boolean };

export type LinkMessage =
	| {
			kind: "hello" | "alive";
			tab: string;
			watching: Watching | null;
			wish: Wish | null;
			open: string | null;
	  }
	| { kind: "bye"; tab: string }
	| { kind: "refetch"; tab: string; organization: string }
	| { kind: "notice"; tab: string; organization: string; notice: LinkNotice };

export type Peer = {
	watching: Watching | null;
	wish: Wish | null;
	open: string | null;
	seen: number;
};

export type LinkChannel = {
	post: (message: LinkMessage) => void;
	subscribe: (listener: (message: LinkMessage) => void) => () => void;
	close: () => void;
};

const NAME = "kestrel-operator-link-v1";

export function sharedChannel(): LinkChannel | null {
	if (typeof window === "undefined" || typeof BroadcastChannel === "undefined") return null;
	const channel = new BroadcastChannel(NAME);

	return {
		// oxlint-disable-next-line unicorn/require-post-message-target-origin -- this is a BroadcastChannel, whose messages reach only this origin.
		post: (message) => channel.postMessage(message),
		subscribe: (listener) => {
			const handler = (event: MessageEvent<LinkMessage>) => listener(event.data);
			channel.addEventListener("message", handler);
			return () => channel.removeEventListener("message", handler);
		},
		close: () => channel.close(),
	};
}
