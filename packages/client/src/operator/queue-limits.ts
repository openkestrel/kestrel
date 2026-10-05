import type { Queue } from "./generated";

function namesSuffix(names: string[]): string {
	return names.length > 0 ? `: ${names.join(", ")}` : "";
}

export function slotsLine(queue: Queue): string {
	const { limit, occupied, occupants, elsewhere } = queue.active_work;
	const local = namesSuffix(occupants.map((occupant) => occupant.name));
	const foreign = elsewhere > 0 ? ` · ${elsewhere} elsewhere` : "";
	if (limit === null) {
		return `Slots no dispatch configuration recorded · ${occupied} occupied${local}${foreign}`;
	}
	return `Slots ${occupied}/${limit}${local}${foreign}`;
}

export function instancesLine(queue: Queue): string {
	const { limit, count, counted } = queue.instances;
	const ceiling = limit === null ? " (no limit)" : `/${limit}`;
	return `Instances ${count}${ceiling}${namesSuffix(counted)}`;
}
