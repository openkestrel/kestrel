import type { Action, Diagnostic, Resource } from "./generated";
import { operatorPath } from "./transport";

export type Input = { name: string; label: string; secret: boolean };

export type Collected = Partial<Record<string, string>>;

export type WriteRequest = { method: "POST" | "PUT"; path: string; body?: unknown };

export type Step =
	| {
			kind: "write";
			label: string;
			destructive: boolean;
			consequence?: string;
			inputs: Input[];
			request: (values: Collected) => WriteRequest;
	  }
	| { kind: "link"; label: string; link: Link }
	| { kind: "reread"; label: string; detail?: string; waitSeconds?: number; retry: boolean }
	| { kind: "field"; label: string; field: string }
	| { kind: "guidance"; label: string; detail?: string };

export type Link =
	| { to: "/" }
	| { to: "/organizations/$organization"; params: { organization: string } }
	| {
			to: "/organizations/$organization/workspaces/$workspace";
			params: { organization: string; workspace: string };
	  };

const RESOURCES: Record<Resource, [one: string, many: string]> = {
	organization: ["Organization", "Organizations"],
	project: ["Project", "Projects"],
	agent: ["Agent", "Agents"],
	subscription_profile: ["Subscription Profile", "Subscription Profiles"],
	provider_credential: ["Provider Credential", "Provider Credentials"],
	integration: ["Integration", "Integrations"],
	trigger: ["Trigger", "Triggers"],
	event: ["Event", "Events"],
	workspace: ["Workspace", "Workspaces"],
	session: ["Session", "Sessions"],
	instance: ["Instance", "Instances"],
	held_message: ["Held Message", "Held Messages"],
	transcript_payload: ["Transcript payload", "Transcript payloads"],
};

export function resourceName(resource: Resource): string {
	return RESOURCES[resource][0];
}

function harnessName(harness: string | null): string {
	return `the ${harness ?? "agent's"} harness`;
}

function within(organization: string | null): string {
	return organization === null ? "" : ` in ${organization}`;
}

const LABELS: Record<string, string> = {
	name: "Name",
	repositories: "Repositories",
	branch: "Branch",
	harness: "Harness",
	owner: "Owner",
	agent: "Agent",
	model: "Model",
	mode: "Mode",
	thought_level: "Thought level",
};

function inputsFor(missing: string[]): Input[] {
	return missing.map((name) => ({ name, label: LABELS[name] ?? name, secret: false }));
}

function declaration(
	resource: Resource,
	action: { name: string | null; missing: string[] },
	organization: string | null,
	path: string,
	body: (values: Collected) => Record<string, unknown>,
): Step {
	return {
		kind: "write",
		label: `Declare the ${resourceName(resource)}${action.name === null ? "" : ` ${action.name}`}${within(organization)}`,
		destructive: false,
		inputs: inputsFor(action.missing),
		request: (values) => ({
			method: "POST",
			path,
			body: { name: action.name ?? values.name, ...body(values) },
		}),
	};
}

export function stepOf(action: Action, origin: string): Step {
	switch (action.action) {
		case "inspect_resource": {
			const label =
				`Inspect the ${resourceName(action.resource)} ${action.reference ?? ""}`.trimEnd();
			if (action.resource === "organization" && action.reference !== null) {
				return {
					kind: "link",
					label,
					link: { to: "/organizations/$organization", params: { organization: action.reference } },
				};
			}
			if (
				action.resource === "workspace" &&
				action.reference !== null &&
				action.organization !== null
			) {
				return {
					kind: "link",
					label,
					link: {
						to: "/organizations/$organization/workspaces/$workspace",
						params: { organization: action.organization, workspace: action.reference },
					},
				};
			}
			return { kind: "guidance", label: `${label}${within(action.organization)}` };
		}
		case "list_resources": {
			const label = `List the ${RESOURCES[action.resource][1]}${within(action.organization)}`;
			if (action.resource === "organization") return { kind: "link", label, link: { to: "/" } };
			if (action.resource === "workspace" && action.organization !== null) {
				return {
					kind: "link",
					label,
					link: {
						to: "/organizations/$organization",
						params: { organization: action.organization },
					},
				};
			}
			return { kind: "guidance", label };
		}
		case "retry_read":
			return {
				kind: "reread",
				label: "Read again",
				...(action.retry_after_seconds !== null && { waitSeconds: action.retry_after_seconds }),
				retry: true,
			};
		case "inspect_operation":
			return {
				kind: "reread",
				label: "Read what is there now",
				...(action.uncertain && {
					detail: "It may have taken effect; check before trying it again.",
				}),
				retry: false,
			};
		case "check_connection":
			return {
				kind: "reread",
				label: "Check the connection again",
				detail: `Check that the control plane at ${origin} is running and reachable from this browser.${
					action.compose ? " Its Docker Compose services should all be up." : ""
				}`,
				retry: false,
			};
		case "correct_field":
			return {
				kind: "field",
				label: `Correct ${action.field}: ${
					action.allowed_values === null
						? spaced(action.constraint)
						: `one of ${action.allowed_values.join(", ")}`
				}`,
				field: action.field,
			};
		case "declare_project":
			return declaration(
				"project",
				action,
				action.organization,
				operatorPath("organizations", action.organization, "projects"),
				(values) => ({
					repositories:
						action.repositories ??
						(values.repositories ?? "").split(/[\s,]+/).filter((repository) => repository !== ""),
					branch: action.branch ?? values.branch,
				}),
			);
		case "set_provider_credential":
			return {
				kind: "write",
				label: `Set the Provider Credential ${action.name} in ${action.organization}`,
				destructive: false,
				inputs: [{ name: "secret", label: "Value", secret: true }],
				request: (values) => ({
					method: "PUT",
					path: operatorPath("organizations", action.organization, "credentials", action.name),
					body: { secret: values.secret },
				}),
			};
		case "stop_session":
			return {
				kind: "write",
				label: `Stop the Session ${action.session}`,
				destructive: action.requires_choice,
				consequence: action.consequence,
				inputs: [],
				request: () => ({
					method: "POST",
					path: operatorPath(
						"organizations",
						action.organization,
						"sessions",
						action.session,
						"stop",
					),
				}),
			};
		case "release_instance":
			return {
				kind: "write",
				label: `Release the Instance of ${action.workspace}`,
				destructive: action.requires_choice,
				consequence: action.consequence,
				inputs: [],
				request: () => ({
					method: "POST",
					path: operatorPath(
						"organizations",
						action.organization,
						"workspaces",
						action.workspace,
						"instance",
						"release",
					),
					body: {},
				}),
			};
		case "declare_organization":
			return declaration("organization", action, null, operatorPath("organizations"), () => ({}));
		case "declare_agent":
			return declaration(
				"agent",
				action,
				action.organization,
				operatorPath("organizations", action.organization, "agents"),
				(values) => ({ harness: action.harness ?? values.harness }),
			);
		case "declare_subscription_profile":
			return declaration(
				"subscription_profile",
				action,
				action.organization,
				operatorPath("organizations", action.organization, "profiles"),
				(values) => ({ owner: action.owner ?? values.owner }),
			);
		case "name_operator":
			return {
				kind: "write",
				label: "Name the Operator",
				destructive: false,
				inputs: inputsFor(action.missing),
				request: (values) => ({
					method: "PUT",
					path: operatorPath("operator"),
					body: { name: action.name ?? values.name },
				}),
			};
		case "enqueue_session":
			return {
				kind: "write",
				label: `Enqueue a Session in ${action.workspace}`,
				destructive: false,
				inputs: inputsFor(action.missing),
				request: (values) => ({
					method: "POST",
					path: operatorPath(
						"organizations",
						action.organization,
						"workspaces",
						action.workspace,
						"sessions",
					),
					body: Object.fromEntries(
						action.missing.flatMap((name) => (values[name] ? [[name, values[name]]] : [])),
					),
				}),
			};
		case "enable_integration":
			return {
				kind: "write",
				label: `Enable the Integration ${action.integration}`,
				destructive: false,
				inputs: [],
				request: () => ({
					method: "POST",
					path: operatorPath(
						"organizations",
						action.organization,
						"integrations",
						action.integration,
						"enable",
					),
					body: {},
				}),
			};
		case "sign_in":
			return {
				kind: "guidance",
				label: `Sign in to ${harnessName(action.harness)}${
					action.method === null ? "" : ` with ${action.method}`
				}`,
			};
		case "inspect_harness_image":
			return {
				kind: "guidance",
				label: `Inspect ${harnessName(action.harness)}'s image${
					action.image === null ? "" : ` ${action.image}`
				}`,
				...(action.command !== null && { detail: `It should provide ${action.command}.` }),
			};
		default:
			// A step from a newer control plane: reading is always safe, guessing a write is not.
			return { kind: "reread", label: "Read what is there now", retry: false };
	}
}

export type Fact = [term: string, detail: string];

function spaced(code: string): string {
	return code.replaceAll("_", " ");
}

function present(...facts: [string, string | null | undefined | false][]): Fact[] {
	return facts.filter((fact): fact is Fact => typeof fact[1] === "string" && fact[1] !== "");
}

export function factsOf(diagnostic: Diagnostic): Fact[] {
	const field = present(["Field", diagnostic.field]);
	switch (diagnostic.kind) {
		case "missing_reference":
		case "expired_resource":
			return [
				...present([resourceName(diagnostic.context.resource), diagnostic.context.reference]),
				...field,
			];
		case "ambiguous_reference":
			return [
				[resourceName(diagnostic.context.resource), diagnostic.context.reference],
				...diagnostic.context.candidates.map(({ id, name }): Fact => [
					"Could mean",
					`${name} (${id})`,
				]),
			];
		case "forbidden_action":
			return present(
				[resourceName(diagnostic.context.resource), diagnostic.context.reference],
				["Constraint", diagnostic.context.constraint && spaced(diagnostic.context.constraint)],
				["Field", diagnostic.field],
			);
		case "state_conflict":
			return present(
				[resourceName(diagnostic.context.resource), diagnostic.context.reference],
				["State", spaced(diagnostic.context.state)],
				["Held by", diagnostic.context.holding_session],
				["Field", diagnostic.field],
			);
		case "invalid_field":
			return present(
				["Field", diagnostic.context.field],
				["Allowed", diagnostic.context.allowed_values?.join(", ")],
			);
		case "malformed_request":
			return field;
		case "setup_gap":
			return present(
				["Needs", spaced(diagnostic.context.prerequisite)],
				[
					diagnostic.context.resource === null ? "" : resourceName(diagnostic.context.resource),
					diagnostic.context.reference,
				],
				["Harness", diagnostic.context.harness],
				["Sign-in method", diagnostic.context.method],
				["Sign-in", diagnostic.context.sign_in],
			);
		case "unavailable":
			return present([
				"Ask again after",
				diagnostic.context.retry_after_seconds !== null &&
					`${diagnostic.context.retry_after_seconds} s`,
			]);
		case "instance_timeout":
			return present(
				["Workspace", diagnostic.context.workspace],
				["Instance", diagnostic.context.instance],
			);
		case "authentication_failed": {
			const { context } = diagnostic;
			return present(
				["Harness", context.harness],
				["Image", context.image],
				["Sign-in methods offered", context.evidence.methods.join(", ")],
				["Sign-in method", context.evidence.method],
				["Sign-in", context.sign_in],
				["Expired", context.expired === true && "yes"],
				["Covers this harness", context.covered === false && "no"],
			);
		}
		case "executable_missing": {
			const { context } = diagnostic;
			const { error } = context.evidence;
			return present(
				["Harness", context.harness],
				["Image", context.image],
				["Executable", context.executable],
				["Error", `${spaced(error.kind)}${error.code === null ? "" : ` (${error.code})`}`],
			);
		}
		case "unknown_failure":
			return present(["The agent said", diagnostic.context.evidence?.summary]);
		case "connection_failed":
			return present(["Control plane", diagnostic.context.url]);
		case "client_failure":
			return present(["Evidence", diagnostic.context.evidence]);
		case "unknown_response":
			return present(
				["Status", diagnostic.context.status?.toString()],
				["Evidence", diagnostic.context.evidence],
				["Field", diagnostic.field],
			);
		default:
			return field;
	}
}
