import { type UseQueryResult, useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { CodeBlock } from "#/components/ai-elements/code-block";
import { languageFor } from "#/components/ai-elements/code-language";
import { FileTree, FileTreeFile, FileTreeFolder } from "#/components/ai-elements/file-tree";
import { Button } from "#/components/ui/button";
import { Toggle } from "#/components/ui/toggle";
import { size } from "#/operator/format";
import type { FileEntry, FileListing } from "#/operator/generated";
import { fileQuery, filesQuery } from "#/operator/work-queries";
import { childPath, fileKindLabel } from "#/operator/work-view";
import type { Row } from "../row";
import { WorkRead } from "./work-read";

export function FilesTab(row: Row) {
	const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
	const [open, setOpen] = useState<string | null>(null);
	const [raw, setRaw] = useState(false);
	const root = useQuery(filesQuery(row.organization, row.workspace, ""));

	const toggle = (path: string) =>
		setExpanded((all) => {
			const next = new Set(all);
			if (next.has(path)) next.delete(path);
			else next.add(path);
			return next;
		});
	const showing = (path: string) => {
		setOpen(path);
		setRaw(false);
	};

	return (
		<div className="grid gap-4 p-4 text-sm">
			{root.isSuccess && root.data.entries.length > 0 && (
				<FileTree
					expanded={expanded}
					onExpandedChange={setExpanded}
					onSelect={toggle}
					selectedPath={open ?? undefined}
				>
					{root.data.entries.map((entry) => (
						<Entry
							key={entry.name}
							parent=""
							entry={entry}
							expanded={expanded}
							onOpen={showing}
							{...row}
						/>
					))}
				</FileTree>
			)}
			<ListingState listing={root} />

			{open !== null && (
				<OpenFile path={open} raw={raw} onRaw={setRaw} onClose={() => setOpen(null)} {...row} />
			)}
		</div>
	);
}

// A tree holds only treeitems and groups, so a listing's own states are drawn outside both.
function ListingState({ listing }: { listing: UseQueryResult<FileListing> }) {
	return (
		<WorkRead read={listing}>
			{({ entries, total, truncated }) =>
				entries.length === 0 ? (
					<p className="text-muted-foreground text-xs" data-empty>
						This directory is empty.
					</p>
				) : (
					truncated && (
						<p className="text-muted-foreground text-xs" data-truncated>
							showing {entries.length} of {total} entries
						</p>
					)
				)
			}
		</WorkRead>
	);
}

type Listed = {
	parent: string;
	entry: FileEntry;
	expanded: Set<string>;
	onOpen: (path: string) => void;
} & Row;

function Entry({ parent, entry, onOpen, ...rest }: Listed) {
	const path = childPath(parent, entry.name);

	if (entry.kind === "directory") {
		return <Folder parent={parent} entry={entry} onOpen={onOpen} {...rest} />;
	}

	return (
		<FileTreeFile
			data-path={path}
			path={path}
			name={entry.name}
			onClick={() => onOpen(path)}
			onKeyDown={(event) => {
				if (event.key !== "Enter" && event.key !== " ") return;
				event.preventDefault();
				onOpen(path);
			}}
		>
			<span className="min-w-0 truncate">{entry.name}</span>
			<span className="ml-auto shrink-0 pl-2 text-muted-foreground text-xs">
				{fileKindLabel(entry)}
				{entry.size === undefined ? "" : ` · ${size(entry.size)}`}
			</span>
		</FileTreeFile>
	);
}

function Folder({ parent, entry, expanded, onOpen, ...row }: Listed) {
	const path = childPath(parent, entry.name);
	const open = expanded.has(path);
	const listing = useQuery({
		...filesQuery(row.organization, row.workspace, path),
		enabled: open,
	});

	return (
		<FileTreeFolder
			data-path={path}
			path={path}
			name={entry.name}
			status={open && <ListingState listing={listing} />}
		>
			{open &&
				listing.isSuccess &&
				listing.data.entries.map((child) => (
					<Entry
						key={child.name}
						parent={path}
						entry={child}
						expanded={expanded}
						onOpen={onOpen}
						{...row}
					/>
				))}
		</FileTreeFolder>
	);
}

function OpenFile({
	organization,
	workspace,
	path,
	raw,
	onRaw,
	onClose,
}: {
	path: string;
	raw: boolean;
	onRaw: (raw: boolean) => void;
	onClose: () => void;
} & Row) {
	const file = useQuery(fileQuery(organization, workspace, path, raw));

	return (
		<section
			data-file={path}
			aria-busy={file.isPending}
			className="grid gap-2 rounded-md border p-3"
		>
			<header className="flex flex-wrap items-center gap-2">
				<h3 className="min-w-0 flex-1 truncate font-medium text-xs">{path}</h3>
				<Toggle type="button" size="sm" variant="outline" pressed={raw} onPressedChange={onRaw}>
					Raw
				</Toggle>
				<Button type="button" variant="ghost" size="icon" aria-label="Close file" onClick={onClose}>
					<X aria-hidden />
				</Button>
			</header>
			<WorkRead read={file}>
				{(data) =>
					data.kind === "text" ? (
						<CodeBlock className="text-xs" code={data.text} language={languageFor(path)} />
					) : (
						<p data-binary className="text-muted-foreground text-xs">
							binary content · {size(data.bytes)}
						</p>
					)
				}
			</WorkRead>
		</section>
	);
}
