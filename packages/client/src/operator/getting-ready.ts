import type { NewWorkspaceDraft } from "#/lib/new-workspace-draft";
import type { Session, Workspace } from "./generated";
import { operator } from "./queries";
import { operatorPath } from "./transport";

export function preparingStep(preparing: Session["preparing"] | undefined): string {
	switch (preparing) {
		case "provisioning":
			return "provisioning the Instance";
		case "cloning":
			return "cloning the checkout";
		case "starting_harness":
			return "starting the harness";
		case "harness_ready":
			return "the harness is ready";
		default:
			return "preparing";
	}
}

// What the Session is doing while a person writes its first message, or why it stopped.
export function sessionStatusLine(session: Session | undefined): string | undefined {
	if (!session) return undefined;
	switch (session.state) {
		case "unbriefed":
			return session.preparing === "harness_ready"
				? "Ready: the harness is up, and your first message becomes the Brief."
				: `Getting ready: ${preparingStep(session.preparing)}…`;
		case "ended":
			return session.exit?.status === "failed"
				? `The Session failed: ${session.exit.because ?? "it failed"}.`
				: undefined;
		case "unreachable":
			return "The Session is unreachable: its supervisor was lost.";
		default:
			return undefined;
	}
}

// What the New Workspace form is handed when a person carries a sealed Workspace on.
export function continueDraft(record: Workspace): Partial<NewWorkspaceDraft> {
	return {
		project: record.project,
		agent: record.opened_with,
		profile: record.profile ?? "",
		branch: record.checkout.branch,
		continues: record.id,
		options: true,
	};
}

export async function postMessage(
	organization: string,
	workspace: string,
	participant: string,
	message: string,
): Promise<Session | null> {
	return operator.write<Session | null>(
		"POST",
		operatorPath("organizations", organization, "workspaces", workspace, "messages"),
		{ participant, message },
	);
}
