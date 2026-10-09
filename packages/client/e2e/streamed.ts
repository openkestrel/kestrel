import type { Page } from "@playwright/test";

export type Subscribed = {
	token: string;
	id: string;
	kind: "notices" | "transcript";
	organization: string;
	workspace?: string;
	after?: string;
	participant?: string;
};

// Answers with the per-resource stream's text for this subscription's `attempt`th subscribe.
type Answer = (subscription: Subscribed, attempt: number) => string | Promise<string>;

// Each connection closes once answered, so the tab reserves and re-subscribes for the next attempt.
export class Streamed {
	readonly subscriptions: Subscribed[] = [];
	readonly opened: string[] = [];
	private reserved = 0;
	private readonly held = new Map<string, Map<string, Subscribed>>();
	private readonly waiting = new Map<string, PromiseWithResolvers<void>>();

	constructor(private readonly answers: { transcript?: Answer; notices?: Answer } = {}) {}

	async install(page: Page): Promise<void> {
		await page.route(
			(url) => url.pathname.startsWith("/operator/streams"),
			async (route) => {
				const request = route.request();
				const [, , , token = "", , id = ""] = new URL(request.url()).pathname.split("/");
				if (token === "") {
					this.reserved += 1;
					const reserved = `mock-${this.reserved}`;
					this.held.set(reserved, new Map());
					await route.fulfill({ status: 201, json: { token: reserved } });
					return;
				}
				if (request.method() === "PUT") {
					// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the Client sends the generated subscription.
					const subscribed = { token, id, ...request.postDataJSON() } as Subscribed;
					this.subscriptions.push(subscribed);
					this.held.get(token)?.set(id, subscribed);
					this.waiting.get(token)?.resolve();
					await route.fulfill({ status: 204 });
					return;
				}
				if (request.method() === "DELETE") {
					this.held.get(token)?.delete(id);
					await route.fulfill({ status: 204 });
					return;
				}
				this.opened.push(token);
				await route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: await this.connection(token),
				});
			},
		);
	}

	private async connection(token: string): Promise<string> {
		const held = this.held.get(token) ?? new Map<string, Subscribed>();
		if (held.size === 0) {
			const waiting = Promise.withResolvers<void>();
			this.waiting.set(token, waiting);
			await waiting.promise;
		}
		// The tab subscribes beside opening, so its other subscriptions are a moment behind.
		await new Promise((resolve) => setTimeout(resolve, 100));
		const bodies = await Promise.all(
			[...held.values()].map(async (subscribed) => {
				const attempt = this.subscriptions.filter(
					(earlier) =>
						earlier.kind === subscribed.kind && earlier.workspace === subscribed.workspace,
				).length;
				const answer =
					subscribed.kind === "transcript"
						? (this.answers.transcript ?? (() => ""))
						: (this.answers.notices ??
							((_, nth) => (nth === 1 ? "event: open\ndata: {}\n\n" : "")));
				return framed(await answer(subscribed, attempt), subscribed.id);
			}),
		);
		return bodies.join("");
	}
}

function framed(text: string, subscription: string): string {
	return text
		.split("\n\n")
		.filter((block) => block.trim() !== "")
		.map((block) => {
			let event = "message";
			let cursor: string | undefined;
			const data: string[] = [];
			for (const line of block.split("\n")) {
				if (line.startsWith("event:")) event = line.slice(6).trim();
				else if (line.startsWith("id:")) cursor = line.slice(3).trim();
				else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
			}
			const envelope = {
				subscription,
				...(cursor ? { cursor } : {}),
				data: parsed(data.join("\n")),
			};
			return `event: ${event}\ndata: ${JSON.stringify(envelope)}\n\n`;
		})
		.join("");
}

function parsed(data: string): unknown {
	try {
		return JSON.parse(data);
	} catch {
		return data;
	}
}
