import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { CodeBlock } from "#/components/ai-elements/code-block";
import { FileTree, FileTreeFile, FileTreeFolder } from "#/components/ai-elements/file-tree";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import { Toggle } from "#/components/ui/toggle";
import { size } from "#/operator/format";
import type { FileEntry } from "#/operator/generated";
import { fileQuery, filesQuery } from "#/operator/work-queries";
import { childPath, fileKindLabel, languageFor } from "#/operator/work-view";

export function FilesTab({ organization, workspace }: { organization: string; workspace: string }) {
	const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
	const [open, setOpen] = useState<string | null>(null);
	const [raw, setRaw] = useState(false);
	const root = useQuery(filesQuery(organization, workspace, ""));

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

	// A tree holds only treeitems and groups, so the read's own states stand outside it.
	return (
		<div className="grid gap-4 p-4 text-sm">
			{root.isPending ? (
				<Skeleton className="h-8 w-full" />
			) : root.isError ? (
				<Refusal error={root.error} retry={() => void root.refetch()} />
			) : root.data.entries.length === 0 ? (
				<p className="text-muted-foreground text-xs" data-empty>
					This directory is empty.
				</p>
			) : (
				<>
					<FileTree
						expanded={expanded}
						onExpandedChange={setExpanded}
						onSelect={toggle}
						selectedPath={open ?? undefined}
					>
						{root.data.entries.map((entry) => (
							<Entry
								key={entry.name}
								organization={organization}
								workspace={workspace}
								parent=""
								entry={entry}
								expanded={expanded}
								onOpen={showing}
							/>
						))}
					</FileTree>
					{root.data.truncated && (
						<p className="text-muted-foreground text-xs" data-truncated>
							showing {root.data.entries.length} of {root.data.total} entries
						</p>
					)}
				</>
			)}

			{open !== null && (
				<OpenFile
					organization={organization}
					workspace={workspace}
					path={open}
					raw={raw}
					onRaw={setRaw}
					onClose={() => setOpen(null)}
				/>
			)}
		</div>
	);
}

function Directory({
	organization,
	workspace,
	path,
	expanded,
	onOpen,
}: {
	organization: string;
	workspace: string;
	path: string;
	expanded: Set<string>;
	onOpen: (path: string) => void;
}) {
	const listing = useQuery(filesQuery(organization, workspace, path));

	if (listing.isPending) return <Skeleton className="h-8 w-full" />;
	if (listing.isError)
		return <Refusal error={listing.error} retry={() => void listing.refetch()} />;
	if (listing.data.entries.length === 0) {
		return <p className="text-muted-foreground text-xs">This directory is empty.</p>;
	}

	return (
		<>
			{listing.data.entries.map((entry) => (
				<Entry
					key={entry.name}
					organization={organization}
					workspace={workspace}
					parent={path}
					entry={entry}
					expanded={expanded}
					onOpen={onOpen}
				/>
			))}
			{listing.data.truncated && (
				<p className="text-muted-foreground text-xs" data-truncated>
					showing {listing.data.entries.length} of {listing.data.total} entries
				</p>
			)}
		</>
	);
}

function Entry({
	organization,
	workspace,
	parent,
	entry,
	expanded,
	onOpen,
}: {
	organization: string;
	workspace: string;
	parent: string;
	entry: FileEntry;
	expanded: Set<string>;
	onOpen: (path: string) => void;
}) {
	const path = childPath(parent, entry.name);

	if (entry.kind === "directory") {
		return (
			<FileTreeFolder data-path={path} path={path} name={entry.name}>
				{expanded.has(path) && (
					<Directory
						organization={organization}
						workspace={workspace}
						path={path}
						expanded={expanded}
						onOpen={onOpen}
					/>
				)}
			</FileTreeFolder>
		);
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

function OpenFile({
	organization,
	workspace,
	path,
	raw,
	onRaw,
	onClose,
}: {
	organization: string;
	workspace: string;
	path: string;
	raw: boolean;
	onRaw: (raw: boolean) => void;
	onClose: () => void;
}) {
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
			{file.isPending ? (
				<Skeleton className="h-8 w-full" />
			) : file.isError ? (
				<Refusal error={file.error} retry={() => void file.refetch()} />
			) : file.data.kind === "text" ? (
				<CodeBlock className="text-xs" code={file.data.text} language={languageFor(path)} />
			) : (
				<p data-binary className="text-muted-foreground text-xs">
					binary content · {size(file.data.bytes)}
				</p>
			)}
		</section>
	);
}
