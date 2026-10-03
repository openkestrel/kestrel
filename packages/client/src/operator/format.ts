import type { QueueReason, Session } from "./generated";

export function ago(timestamp: string, now: number = Date.now()): string {
	const at = Date.parse(timestamp);
	if (Number.isNaN(at)) return "at an unknown time";
	const seconds = Math.max(0, Math.round((now - at) / 1000));
	if (seconds < 45) return "just now";
	if (seconds < 90) return "a minute ago";
	const minutes = Math.round(seconds / 60);
	if (minutes < 60) return `${minutes} minutes ago`;
	const hours = Math.round(minutes / 60);
	if (hours < 24) return `${hours} hours ago`;
	return `${Math.round(hours / 24)} days ago`;
}

export function size(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KiB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

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

export function reasonText(reason: QueueReason): string {
	switch (reason.kind) {
		case "dependencies":
			return `waiting on ${list(reason.sessions)}`;
		case "subscription_profile":
			return `${reason.session ?? "a Session in another Organization"} holds the ${reason.profile} profile`;
		case "instance_archiving":
			return `archiving ${reason.instance} to make room`;
		case "live_instance_limit":
			return `at the live Instance limit of ${reason.limit}`;
		case "active_work_slots":
			return `every Active-Work Slot is occupied (${reason.limit})`;
		case "ahead":
			return `behind ${list([
				...reason.sessions,
				...((reason.elsewhere ?? 0) > 0
					? [
							`${reason.elsewhere} Session${reason.elsewhere === 1 ? "" : "s"} in other Organizations`,
						]
					: []),
			])}`;
		default: {
			const unhandled: never = reason;
			throw new Error(`no such queue reason: ${String(unhandled)}`);
		}
	}
}

function list(names: string[]): string {
	if (names.length === 0) return "nothing";
	if (names.length === 1) return names[0] ?? "nothing";
	return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}
