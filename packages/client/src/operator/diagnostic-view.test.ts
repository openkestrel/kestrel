import { describe, expect, it } from "vitest";
import { factsOf, stepOf } from "./diagnostic-view";
import type { Action, Diagnostic } from "./generated";

const ORIGIN = "https://kestrel.example";

describe("a step that declares a missing record", () => {
	it("collects only the inputs it is missing, in the Organization it names", () => {
		const step = stepOf(
			{
				action: "declare_project",
				organization: "acme",
				name: "kestrel",
				repositories: null,
				branch: null,
				missing: ["repositories", "branch"],
			},
			ORIGIN,
		);

		expect(step).toMatchObject({
			kind: "write",
			label: "Declare the Project kestrel in acme",
			destructive: false,
			inputs: [
				{ name: "repositories", label: "Repositories", secret: false },
				{ name: "branch", label: "Branch", secret: false },
			],
		});
		if (step.kind !== "write") throw new Error("not a write");
		expect(
			step.request({
				repositories: "https://github.com/a/b, https://github.com/a/c",
				branch: "main",
			}),
		).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/projects",
			body: {
				name: "kestrel",
				repositories: ["https://github.com/a/b", "https://github.com/a/c"],
				branch: "main",
			},
		});
	});
});

describe("a step that sets a Provider Credential", () => {
	it("collects its value privately and names it nowhere but the request", () => {
		const step = stepOf(
			{ action: "set_provider_credential", organization: "acme", name: "ANTHROPIC_API_KEY" },
			ORIGIN,
		);

		expect(step).toMatchObject({
			kind: "write",
			label: "Set the Provider Credential ANTHROPIC_API_KEY in acme",
			inputs: [{ name: "secret", label: "Value", secret: true }],
		});
		if (step.kind !== "write") throw new Error("not a write");
		expect(step.request({ secret: "sk-1" })).toEqual({
			method: "PUT",
			path: "/operator/organizations/acme/credentials/ANTHROPIC_API_KEY",
			body: { secret: "sk-1" },
		});
	});
});

describe("a destructive step", () => {
	it("is a choice that shows its consequence, whatever the consequence says", () => {
		const step = stepOf(
			{
				action: "stop_session",
				organization: "acme",
				session: "calm-river",
				consequence: "Nothing much.",
				effect: "fails_session",
				requires_choice: true,
			},
			ORIGIN,
		);

		expect(step).toMatchObject({
			kind: "write",
			label: "Stop the Session calm-river",
			destructive: true,
			consequence: "Nothing much.",
			inputs: [],
		});
		if (step.kind !== "write") throw new Error("not a write");
		expect(step.request({})).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/sessions/calm-river/stop",
		});
	});

	it("releases the Instance a Workspace holds only by the same explicit choice", () => {
		const step = stepOf(
			{
				action: "release_instance",
				organization: "acme",
				workspace: "brave-otter",
				instance: "i-1",
				consequence: "Releasing the instance discards work that may exist nowhere else.",
				effect: "discards_unpublished_work",
				requires_choice: true,
			},
			ORIGIN,
		);

		expect(step).toMatchObject({
			kind: "write",
			label: "Release the Instance of brave-otter",
			destructive: true,
		});
		if (step.kind !== "write") throw new Error("not a write");
		expect(step.request({})).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/workspaces/brave-otter/instance/release",
			body: {},
		});
	});
});

describe("an inspection", () => {
	it("opens the Workspace it names in the Organization it names", () => {
		expect(
			stepOf(
				{
					action: "inspect_resource",
					resource: "workspace",
					reference: "brave-otter",
					organization: "acme",
				},
				ORIGIN,
			),
		).toEqual({
			kind: "link",
			label: "Inspect the Workspace brave-otter",
			link: {
				to: "/organizations/$organization/workspaces/$workspace",
				params: { organization: "acme", workspace: "brave-otter" },
			},
			command: `kestrel workspace show brave-otter --organization acme --control-plane ${ORIGIN}`,
		});
	});

	it("lists an Organization's Workspaces, or every Organization", () => {
		expect(
			stepOf({ action: "list_resources", resource: "workspace", organization: "acme" }, ORIGIN),
		).toMatchObject({
			kind: "link",
			label: "List the Workspaces in acme",
			link: { to: "/organizations/$organization", params: { organization: "acme" } },
		});
		expect(
			stepOf({ action: "list_resources", resource: "organization", organization: null }, ORIGIN),
		).toMatchObject({ kind: "link", label: "List the Organizations", link: { to: "/" } });
	});

	it("names a record the browser has no view of rather than inventing one", () => {
		expect(
			stepOf(
				{
					action: "inspect_resource",
					resource: "session",
					reference: "calm-river",
					organization: "acme",
				},
				ORIGIN,
			),
		).toEqual({
			kind: "guidance",
			label: "Inspect the Session calm-river in acme",
			command: `kestrel session show calm-river --organization acme --control-plane ${ORIGIN}`,
		});
	});
});

describe("a step that reads again", () => {
	it("waits as long as the control plane asked", () => {
		expect(
			stepOf(
				{
					action: "retry_read",
					operation: "list_workspaces",
					resource: null,
					retry_after_seconds: 2,
				},
				ORIGIN,
			),
		).toEqual({ kind: "reread", label: "Read again", waitSeconds: 2, retry: true });
	});

	it("after an uncertain write reads, and warns it may have landed", () => {
		expect(
			stepOf(
				{
					action: "inspect_operation",
					operation: "POST /operator/organizations",
					resource: null,
					uncertain: true,
				},
				ORIGIN,
			),
		).toEqual({
			kind: "reread",
			label: "Read what is there now",
			detail: "It may have taken effect; check before trying it again.",
			retry: false,
		});
	});

	it("checks the connection to the control plane this page came from", () => {
		expect(
			stepOf({ action: "check_connection", service: "control_plane", compose: false }, ORIGIN),
		).toEqual({
			kind: "reread",
			label: "Check the connection again",
			detail: `Check that the control plane at ${ORIGIN} is running and reachable from this browser.`,
			retry: false,
		});
	});
});

describe("a field correction", () => {
	it("names the field and the values it allows", () => {
		expect(
			stepOf(
				{
					action: "correct_field",
					operation: "open_workspace",
					resource: null,
					field: "model",
					constraint: "offered",
					allowed_values: ["scripted", "scripted-max"],
				},
				ORIGIN,
			),
		).toEqual({
			kind: "field",
			label: "Correct model: one of scripted, scripted-max",
			field: "model",
		});
	});
});

describe("the other declarations", () => {
	it("collect an Agent's missing harness and a Subscription Profile's missing owner", () => {
		const agent = stepOf(
			{
				action: "declare_agent",
				organization: "acme",
				name: "builder",
				harness: null,
				missing: ["harness"],
			},
			ORIGIN,
		);
		const profile = stepOf(
			{
				action: "declare_subscription_profile",
				organization: "acme",
				name: "team",
				owner: null,
				missing: ["owner"],
			},
			ORIGIN,
		);
		if (agent.kind !== "write" || profile.kind !== "write") throw new Error("not a write");

		expect(agent.label).toBe("Declare the Agent builder in acme");
		expect(agent.request({ harness: "claude" })).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/agents",
			body: { name: "builder", harness: "claude" },
		});
		expect(profile.request({ owner: "jack" })).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/profiles",
			body: { name: "team", owner: "jack" },
		});
	});

	it("declare an Organization and name the Operator", () => {
		const organization = stepOf(
			{ action: "declare_organization", name: "acme", missing: [] },
			ORIGIN,
		);
		const operator = stepOf({ action: "name_operator", name: null, missing: ["name"] }, ORIGIN);
		if (organization.kind !== "write" || operator.kind !== "write") throw new Error("not a write");

		expect(organization).toMatchObject({ label: "Declare the Organization acme", inputs: [] });
		expect(organization.request({})).toEqual({
			method: "POST",
			path: "/operator/organizations",
			body: { name: "acme" },
		});
		expect(operator).toMatchObject({ label: "Name the Operator", inputs: [{ name: "name" }] });
		expect(operator.request({ name: "jack" })).toEqual({
			method: "PUT",
			path: "/operator/operator",
			body: { name: "jack" },
		});
	});
});

describe("enqueueing a Session", () => {
	it("starts one in the Workspace it names, sending only what was collected", () => {
		const step = stepOf(
			{
				action: "enqueue_session",
				organization: "acme",
				workspace: "brave-otter",
				missing: ["agent"],
			},
			ORIGIN,
		);
		if (step.kind !== "write") throw new Error("not a write");

		expect(step).toMatchObject({
			label: "Enqueue a Session in brave-otter",
			destructive: false,
			inputs: [{ name: "agent", label: "Agent" }],
		});
		expect(step.request({ agent: "builder" })).toEqual({
			method: "POST",
			path: "/operator/organizations/acme/workspaces/brave-otter/sessions",
			body: { agent: "builder" },
		});
	});
});

describe("a step outside the browser's screens", () => {
	it("names the sign-in a harness needs", () => {
		expect(
			stepOf(
				{ action: "sign_in", harness: "claude", method: "claude-login", sign_in: null },
				ORIGIN,
			),
		).toEqual({ kind: "guidance", label: "Sign in to the claude harness with claude-login" });
	});

	it("names the executable a harness image should provide", () => {
		expect(
			stepOf(
				{
					action: "inspect_harness_image",
					harness: "claude",
					image: null,
					command: "claude-code-acp",
				},
				ORIGIN,
			),
		).toEqual({
			kind: "guidance",
			label: "Inspect the claude harness's image",
			detail: "It should provide claude-code-acp.",
		});
	});
});

describe("what a diagnostic establishes", () => {
	it("shows a conflict's state and the Session holding it", () => {
		expect(
			factsOf({
				kind: "state_conflict",
				message: "the Workspace is in flight",
				field: null,
				context: {
					operation: "open_workspace",
					resource: "workspace",
					reference: "brave-otter",
					organization: "acme",
					state: "in_flight",
					holding_session: "calm-river",
				},
				next_steps: [],
			}),
		).toEqual([
			["Workspace", "brave-otter"],
			["State", "in flight"],
			["Held by", "calm-river"],
		]);
	});

	it("lists every candidate an ambiguous reference could mean, by name and id", () => {
		expect(
			factsOf({
				kind: "ambiguous_reference",
				message: "more than one Workspace starts brave",
				field: null,
				context: {
					resource: "workspace",
					reference: "brave",
					organization: "acme",
					candidates: [
						{ id: "w-1", name: "brave-otter" },
						{ id: "w-2", name: "brave-heron" },
					],
				},
				next_steps: [],
			}),
		).toEqual([
			["Workspace", "brave"],
			["Could mean", "brave-otter (w-1)"],
			["Could mean", "brave-heron (w-2)"],
		]);
	});

	it("names a failed sign-in's method without calling it expired unless established", () => {
		const facts = factsOf({
			kind: "authentication_failed",
			message: "the claude harness needed a sign-in before it would work",
			field: null,
			context: {
				session: "s-1",
				harness: "claude",
				image: null,
				evidence: {
					kind: "authentication_required",
					code: -32000,
					methods: ["claude-login"],
					method: null,
				},
				sign_in: null,
				expired: null,
				covered: null,
			},
			next_steps: [],
		});

		expect(facts).toEqual([
			["Harness", "claude"],
			["Sign-in methods offered", "claude-login"],
		]);
	});

	it("shows the executable that could not be spawned and why", () => {
		expect(
			factsOf({
				kind: "executable_missing",
				message: "the claude harness's executable claude-code-acp could not be spawned",
				field: null,
				context: {
					session: "s-1",
					harness: "claude",
					image: null,
					executable: "claude-code-acp",
					evidence: {
						kind: "executable_missing",
						command: "claude-code-acp",
						error: { kind: "not_found", code: 2 },
					},
				},
				next_steps: [],
			}),
		).toEqual([
			["Harness", "claude"],
			["Executable", "claude-code-acp"],
			["Error", "not found (2)"],
		]);
	});

	it("shows an unexplained answer's status", () => {
		expect(
			factsOf({
				kind: "unknown_response",
				message: "the control plane answered 502",
				field: null,
				context: { service: "control_plane", operation: "GET /x", status: 502, evidence: null },
				next_steps: [],
			}),
		).toEqual([["Status", "502"]]);
	});
});

describe("a diagnostic newer than this Client", () => {
	it("offers reading what is there now for a step it does not know", () => {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- a step from a newer control plane.
		const unknown = { action: "rotate_keys", organization: "acme" } as unknown as Action;

		expect(stepOf(unknown, ORIGIN)).toEqual({
			kind: "reread",
			label: "Read what is there now",
			retry: false,
		});
	});

	it("shows only its field for a kind it does not know", () => {
		const answered: unknown = {
			kind: "quota_exhausted",
			message: "the quota is spent",
			field: "model",
			context: { quota: 3 },
			next_steps: [],
		};
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- a kind from a newer control plane.
		const unknown = answered as Diagnostic;

		expect(factsOf(unknown)).toEqual([["Field", "model"]]);
	});
});

describe("a recovery command", () => {
	const commandOf = (action: Action) => {
		const step = stepOf(action, ORIGIN);
		return step.kind === "link" || step.kind === "guidance" ? step.command : undefined;
	};
	const at = `--control-plane ${ORIGIN}`;

	it("shows an Organization by its status, and an Event outside any Organization", () => {
		expect(
			commandOf({
				action: "inspect_resource",
				resource: "organization",
				reference: "acme",
				organization: null,
			}),
		).toBe(`kestrel status --organization acme ${at}`);
		expect(
			commandOf({
				action: "inspect_resource",
				resource: "event",
				reference: "e1",
				organization: "acme",
			}),
		).toBe(`kestrel event show e1 ${at}`);
	});

	it("lists what it cannot show by name", () => {
		expect(
			commandOf({
				action: "inspect_resource",
				resource: "instance",
				reference: "i1",
				organization: "acme",
			}),
		).toBe(`kestrel instance list --organization acme ${at}`);
		expect(
			commandOf({
				action: "inspect_resource",
				resource: "workspace",
				reference: null,
				organization: "acme",
			}),
		).toBe(`kestrel workspace list --organization acme ${at}`);
	});

	it("lists a record's kind, with Sessions and payloads under their Workspaces", () => {
		expect(
			commandOf({
				action: "list_resources",
				resource: "subscription_profile",
				organization: "acme",
			}),
		).toBe(`kestrel profile list --organization acme ${at}`);
		expect(commandOf({ action: "list_resources", resource: "session", organization: "acme" })).toBe(
			`kestrel workspace list --organization acme ${at}`,
		);
		expect(
			commandOf({ action: "list_resources", resource: "organization", organization: "acme" }),
		).toBe(`kestrel organization list ${at}`);
	});

	it("quotes a reference the shell would split", () => {
		expect(
			commandOf({
				action: "inspect_resource",
				resource: "session",
				reference: "it's here",
				organization: "acme",
			}),
		).toBe(`kestrel session show 'it'"'"'s here' --organization acme ${at}`);
	});
});
