import { queryOptions } from "@tanstack/react-query";
import type {
	FileListing,
	FileText,
	WorkspaceChanges,
	WorkspaceCommits,
	WorkspaceStashes,
} from "./generated";
import { operator, workspaceKey } from "./queries";
import { operatorPath, type Transport } from "./transport";

// Every live read of one Workspace's Instance, under the Workspace prefix so a Workspace change
// notice refetches what the Work pane shows.
export const instanceKey = (organization: string, workspace: string) =>
	[...workspaceKey(organization, workspace), "instance"] as const;

function instancePath(organization: string, workspace: string, ...segments: string[]): string {
	return operatorPath("organizations", organization, "workspaces", workspace, ...segments);
}

export const changesQuery = (organization: string, workspace: string, scope: string) =>
	queryOptions({
		queryKey: [...instanceKey(organization, workspace), "changes", scope],
		queryFn: ({ signal }) =>
			operator.read<WorkspaceChanges>(
				`${instancePath(organization, workspace, "changes")}?${new URLSearchParams({ scope })}`,
				{ signal },
			),
	});

export const commitsQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: [...instanceKey(organization, workspace), "commits"],
		queryFn: ({ signal }) =>
			operator.read<WorkspaceCommits>(instancePath(organization, workspace, "commits"), {
				signal,
			}),
	});

export const stashesQuery = (organization: string, workspace: string) =>
	queryOptions({
		queryKey: [...instanceKey(organization, workspace), "stashes"],
		queryFn: ({ signal }) =>
			operator.read<WorkspaceStashes>(instancePath(organization, workspace, "stashes"), {
				signal,
			}),
	});

export const filesQuery = (organization: string, workspace: string, path: string) =>
	queryOptions({
		queryKey: [...instanceKey(organization, workspace), "files", path],
		queryFn: ({ signal }) =>
			operator.read<FileListing>(
				`${instancePath(organization, workspace, "files")}?${new URLSearchParams({ path })}`,
				{ signal },
			),
	});

export type FileReading =
	| { kind: "text"; path: string; text: string }
	| { kind: "bytes"; bytes: number };

export const fileQuery = (organization: string, workspace: string, path: string, raw: boolean) =>
	queryOptions({
		queryKey: [...instanceKey(organization, workspace), "file", path, raw],
		queryFn: ({ signal }): Promise<FileReading> =>
			readFile(operator, organization, workspace, path, raw, signal),
	});

export async function readFile(
	operations: Transport,
	organization: string,
	workspace: string,
	path: string,
	raw: boolean,
	signal?: AbortSignal,
): Promise<FileReading> {
	const query = new URLSearchParams({ path });
	if (raw) query.set("raw", "true");
	const { mediaType, body } = await operations.bytes(
		`${instancePath(organization, workspace, "file")}?${query}`,
		{ signal },
	);

	if (mediaType?.includes("application/json")) {
		// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the file read's JSON is the generated FileText.
		const answer = JSON.parse(new TextDecoder().decode(body)) as FileText;
		return { kind: "text", path: answer.path, text: answer.text };
	}

	const text = textual(body);
	if (text !== undefined) return { kind: "text", path, text };
	return { kind: "bytes", bytes: body.byteLength };
}

// Raw bytes are shown only when they decode as text without control characters; anything else is
// reported by size so binary content never reaches the DOM.
function textual(body: ArrayBuffer): string | undefined {
	try {
		const text = new TextDecoder("utf-8", { fatal: true }).decode(body);
		for (const character of text) {
			const code = character.codePointAt(0) ?? 0;
			if (code < 32 && code !== 9 && code !== 10 && code !== 13) return undefined;
		}
		return text;
	} catch {
		return undefined;
	}
}
