import type { Integration, IntegrationState } from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export type Maintenance = "pause" | "key" | "rotate" | "retire";

export type Lifecycle = {
	state: IntegrationState;
	label: string;
	since?: string;
	detail: string;
};

export function lifecycleOf(integration: Integration, organization: string): Lifecycle {
	const scope = `${integration.repository === null ? "" : ` for ${integration.repository}`} in ${organization}`;
	switch (integration.state) {
		case "enabled": {
			const polled =
				integration.polled_every === null ? "" : `, polled every ${integration.polled_every}`;
			return {
				state: "enabled",
				label: "In use",
				detail: `Carries ${integration.carries.join(" and ") || "nothing"}${scope}${polled}.`,
			};
		}
		case "disabled":
			return {
				state: "disabled",
				label: "Paused",
				...(integration.disabled_at !== null && { since: integration.disabled_at }),
				detail: `Paused${scope}: polling, webhook Deliveries, posts and held Firings wait. Its key, secret, poll cursor and pending posts are kept for when it resumes.`,
			};
		case "retired":
			return {
				state: "retired",
				label: "Retired",
				...(integration.retired_at !== null && { since: integration.retired_at }),
				detail: `Retired${scope}, for good and only in kestrel: the key and secret kestrel held are erased and it is never used again. The GitHub App, its installation and its keys are untouched on GitHub; remove them there if you mean to.`,
			};
		default: {
			const unknown: never = integration.state;
			throw new Error(`no such state: ${String(unknown)}`);
		}
	}
}

// Resuming is the producer's step in a paused Integration's diagnostic, and a blocked rotation is the producer's to refuse.
export function offered(integration: Integration): Maintenance[] {
	if (integration.state === "retired") return [];
	const github = integration.kind === "github";
	return [
		...(integration.state === "enabled" ? (["pause"] as const) : []),
		...(github ? (["key", "rotate"] as const) : []),
		"retire",
	];
}

export function consequenceOf(
	maintenance: Exclude<Maintenance, "resume">,
	integration: Integration,
) {
	switch (maintenance) {
		case "pause":
			return "Pausing stops polling, webhook Deliveries, posts back to GitHub and held Firings until it is resumed. Its key, secret, poll cursor and pending posts are kept. Running Sessions carry on, and a request already sent is not recalled.";
		case "key":
			return `GitHub is asked first: the key must sign as this same App, mint for its installation and reach ${integration.repository ?? "its repository"}. Only then is it saved, and no token minted with the previous key is handed out afterwards. A key GitHub refuses saves nothing, and the current key stays in use.`;
		case "rotate":
			return "kestrel makes a new webhook secret, accepts Deliveries signed with either, and sets the new one on the App on GitHub. Until GitHub acknowledges it the rotation stays unfinished and can be resumed here; polling carries on throughout. The secret is never shown.";
		case "retire":
			return "Retiring is permanent. kestrel erases the key and secret it holds and every token minted from them, and cancels the posts not yet made and the Firings held on it. Projects, Triggers, Events and Workspaces that refer to it stay readable, and no other Integration takes its place. Nothing changes on GitHub: the App, its installation and its keys stay until you remove them there.";
		default: {
			const unknown: never = maintenance;
			throw new Error(`no such maintenance: ${String(unknown)}`);
		}
	}
}

export function replaced(
	listed: Integration[] | undefined,
	changed: Integration,
): Integration[] | undefined {
	return listed?.map((integration) => (integration.id === changed.id ? changed : integration));
}

function path(organization: string, integration: Integration, ...rest: string[]) {
	return operatorPath("organizations", organization, "integrations", integration.name, ...rest);
}

// Retiring, rotating and a key name the revision the person saw, so maintenance since refuses them.
export function maintain(
	organization: string,
	integration: Integration,
	maintenance: Exclude<Maintenance, "key">,
): Promise<Integration> {
	switch (maintenance) {
		case "pause":
			return operator.write("POST", path(organization, integration, "disable"));
		case "rotate":
			return operator.write("POST", path(organization, integration, "webhook-secret-rotation"), {
				revision: integration.revision,
			});
		case "retire":
			return operator.write("POST", path(organization, integration, "retire"), {
				revision: integration.revision,
			});
		default: {
			const unknown: never = maintenance;
			throw new Error(`no such maintenance: ${String(unknown)}`);
		}
	}
}

export function replacePrivateKey(
	organization: string,
	integration: Integration,
	privateKey: string,
): Promise<Integration> {
	return operator.write("PUT", path(organization, integration, "private-key"), {
		private_key: privateKey,
		revision: integration.revision,
	});
}
