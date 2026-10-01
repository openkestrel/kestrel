import type { HeldMessage, OptionChange, Posted, Session } from "./generated";
import { operatorPath, type Transport } from "./transport";

function messages(organization: string, workspace: string, id?: number): string {
	return operatorPath(
		"organizations",
		organization,
		"workspaces",
		workspace,
		"messages",
		...(id === undefined ? [] : [String(id)]),
	);
}

export function postTurn(
	operations: Transport,
	organization: string,
	workspace: string,
	participant: string,
	message: string,
): Promise<Posted> {
	return operations.write<Posted>("POST", messages(organization, workspace), {
		participant,
		message,
	});
}

export function editHeldMessage(
	operations: Transport,
	organization: string,
	workspace: string,
	id: number,
	participant: string,
	message: string,
): Promise<HeldMessage> {
	return operations.write<HeldMessage>("PUT", messages(organization, workspace, id), {
		participant,
		message,
	});
}

export function withdrawHeldMessage(
	operations: Transport,
	organization: string,
	workspace: string,
	id: number,
	participant: string,
): Promise<void> {
	return operations.write<void>("DELETE", messages(organization, workspace, id), { participant });
}

export function interruptTurn(
	operations: Transport,
	organization: string,
	session: string,
	participant: string,
): Promise<Session> {
	return operations.write<Session>(
		"POST",
		operatorPath("organizations", organization, "sessions", session, "interrupt"),
		{ participant },
	);
}

export function changeSessionOption(
	operations: Transport,
	organization: string,
	session: string,
	change: OptionChange,
): Promise<Session> {
	return operations.write<Session>(
		"POST",
		operatorPath("organizations", organization, "sessions", session, "options"),
		change,
	);
}
