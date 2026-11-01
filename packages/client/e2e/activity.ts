import { expect, type APIRequestContext, type Page } from "@playwright/test";
import { Streamed } from "./streamed";

export async function prepared(request: APIRequestContext): Promise<void> {
	await request.post("/operator/organizations", { data: { name: "acme" } });
	await request.post("/operator/organizations/acme/projects", {
		data: {
			name: "kestrel",
			repositories: ["https://github.com/openkestrel/kestrel"],
			branch: "main",
		},
	});
	await request.post("/operator/organizations/acme/agents", {
		data: { name: "builder", harness: "opencode" },
	});
}

export type Workspace = { id: string; name: string };

export async function opened(request: APIRequestContext): Promise<Workspace> {
	const response = await request.post("/operator/organizations/acme/workspaces", {
		data: { project: "kestrel", agent: "builder" },
	});
	expect(response.ok(), await response.text()).toBe(true);
	// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the control plane's opened shape.
	const body = (await response.json()) as { workspace: { id: string; name: string } };
	return { id: body.workspace.id, name: body.workspace.name };
}

export type WireEvent = { name: string; id?: string; data: unknown };

export function wire(...events: WireEvent[]): string {
	return events
		.map(
			({ name, id, data }) =>
				`${id ? `id: ${id}\n` : ""}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`,
		)
		.join("");
}

export function entry(workspace: Workspace, seq: number, value: unknown): WireEvent {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "shared_state",
			session_id: null,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: value,
		},
	};
}

export function summary(
	workspace: Workspace,
	first: number,
	last: number,
	overrides: Record<string, unknown> = {},
): WireEvent {
	return {
		name: "activity",
		id: `${workspace.id}:${last}`,
		data: {
			first_seq: first,
			last_seq: last,
			counts: { tool_calls: 0, failed_calls: 0, thoughts: 0, plans: 0, tombstones: 0 },
			tool_kinds: [],
			latest: null,
			started_at: null,
			finished_at: null,
			anomaly: false,
			closed: true,
			...overrides,
		},
	};
}

export function tool(workspace: Workspace, seq: number, overrides: Record<string, unknown> = {}) {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "detail",
			session_id: "00000000-0000-0000-0000-0000000000ff",
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: {
				type: "tool_call",
				session_id: "00000000-0000-0000-0000-0000000000ff",
				call_id: `call-${seq}`,
				title: `tool ${seq}`,
				tool_kind: "execute",
				status: "completed",
				input: {},
				result: { content: [], output: {} },
				closing_reason: null,
				completion: {
					started_at: "2026-09-30T10:00:00Z",
					finished_at: "2026-09-30T10:00:01Z",
					turn_outcome: null,
				},
				...overrides,
			},
		},
	};
}

type RangeAnswer = string | { refused: number };

export const SESSION = "00000000-0000-0000-0000-0000000000ff";
export const COMPLETION = {
	started_at: "2026-09-30T10:00:00Z",
	finished_at: "2026-09-30T10:00:01Z",
	turn_outcome: null,
};

export function narrated(
	workspace: Workspace,
	seq: number,
	value: Record<string, unknown>,
): WireEvent {
	return {
		name: "entry",
		id: `${workspace.id}:${seq}`,
		data: {
			kind: "narration",
			session_id: SESSION,
			seq,
			appended_at: "2026-09-30T10:00:00Z",
			entry: { session_id: SESSION, completion: COMPLETION, ...value },
		},
	};
}

export const caughtUp: WireEvent = { name: "end", data: { because: "caught_up" } };

export function working(tools: unknown[], buffering: { thought?: boolean } = {}): WireEvent {
	return {
		name: "session_state",
		data: {
			session_id: SESSION,
			tools,
			units: [],
			message_buffering: false,
			thought_buffering: buffering.thought ?? false,
			usage: null,
			observation: { availability: "current" },
		},
	};
}

export const stillOpen = () => new Promise<string>(() => {});

export class Scripted {
	readonly ranges: { first: number; last: number; kinds: string | null }[] = [];
	readonly payloads: string[] = [];
	readonly streamed: Streamed;

	private readonly range: (first: number, last: number) => RangeAnswer | Promise<RangeAnswer>;
	private readonly payload: (id: string) => { status: number; body?: string };

	constructor(options: {
		follow: (attempt: number) => string | Promise<string>;
		range?: (first: number, last: number) => RangeAnswer | Promise<RangeAnswer>;
		payload?: (id: string) => { status: number; body?: string };
	}) {
		this.streamed = new Streamed({ transcript: (_, attempt) => options.follow(attempt) });
		this.range = options.range ?? (() => "");
		this.payload = options.payload ?? (() => ({ status: 404 }));
	}

	async install(page: Page): Promise<void> {
		await this.streamed.install(page);
		await page.route(
			(url) => url.pathname.endsWith("/transcript"),
			async (route) => {
				const url = new URL(route.request().url());
				const first = url.searchParams.get("first_seq");
				const last = url.searchParams.get("last_seq");
				this.ranges.push({
					first: Number(first),
					last: Number(last),
					kinds: url.searchParams.get("kinds"),
				});
				const answer = await this.range(Number(first), Number(last));
				if (typeof answer !== "string") {
					await route.fulfill({
						status: answer.refused,
						contentType: "application/json",
						body: JSON.stringify({ message: "the Transcript cannot be read" }),
					});
					return;
				}
				await route.fulfill({ status: 200, contentType: "text/event-stream", body: answer });
			},
		);
		await page.route(
			(url) => url.pathname.includes("/transcript/payloads/"),
			async (route) => {
				const id = decodeURIComponent(
					new URL(route.request().url()).pathname.split("/").at(-1) ?? "",
				);
				this.payloads.push(id);
				const answer = this.payload(id);
				if (answer.status === 200) {
					await route.fulfill({
						status: 200,
						contentType: "text/plain; charset=utf-8",
						body: answer.body ?? "",
					});
					return;
				}
				await route.fulfill({
					status: answer.status,
					contentType: "application/json",
					body: JSON.stringify({ message: "the Transcript payload has expired" }),
				});
			},
		);
	}
}

export async function visiting(page: Page, workspace: Workspace): Promise<void> {
	await page.goto(`/organizations/acme/workspaces/${workspace.name}`);
	await expect(page.getByRole("heading", { name: workspace.name })).toBeVisible();
}
