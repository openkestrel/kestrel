import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Skeleton } from "#/components/ui/skeleton";
import { Toggle } from "#/components/ui/toggle";
import { fileQuery, filesQuery } from "#/operator/work-queries";
import { breadcrumbs, childPath, fileKindLabel, fileSize } from "#/operator/work-view";

export function FilesTab({ organization, workspace }: { organization: string; workspace: string }) {
	const [path, setPath] = useState("");
	const [open, setOpen] = useState<string | null>(null);
	const [raw, setRaw] = useState(false);
	const listing = useQuery(filesQuery(organization, workspace, path));
	const file = useQuery({
		...fileQuery(organization, workspace, open ?? "", raw),
		enabled: open !== null,
	});

	return (
		<div className="grid gap-4 p-4 text-sm">
			<nav aria-label="Path" className="flex flex-wrap items-center gap-1">
				{breadcrumbs(path).map((crumb, index, all) => (
					<span key={crumb.path} className="flex items-center gap-1">
						{index === all.length - 1 ? (
							<span className="text-muted-foreground text-xs">{crumb.label}</span>
						) : (
							<Button
								type="button"
								variant="ghost"
								size="sm"
								onClick={() => {
									setPath(crumb.path);
									setOpen(null);
								}}
							>
								{crumb.label}
							</Button>
						)}
						{index < all.length - 1 && <span className="text-muted-foreground text-xs">/</span>}
					</span>
				))}
			</nav>

			{listing.isPending ? (
				<Skeleton className="h-8 w-full" />
			) : listing.isError ? (
				<Refusal error={listing.error} />
			) : (
				<section className="grid gap-1">
					{listing.data.entries.length === 0 ? (
						<p className="text-muted-foreground text-xs">This directory is empty.</p>
					) : (
						<ul className="grid gap-0.5">
							{listing.data.entries.map((entry) => (
								<li key={entry.name}>
									<button
										type="button"
										className="flex w-full items-baseline gap-2 rounded-md px-2 py-1 text-left hover:bg-accent"
										onClick={() => {
											const next = childPath(path, entry.name);
											if (entry.kind === "directory") {
												setPath(next);
												setOpen(null);
											} else {
												setOpen(next);
												setRaw(false);
											}
										}}
									>
										<span className="truncate">{entry.name}</span>
										<span className="text-muted-foreground text-xs">
											{fileKindLabel(entry)}
											{entry.size !== undefined ? ` · ${fileSize(entry.size)}` : ""}
										</span>
									</button>
								</li>
							))}
						</ul>
					)}
					{listing.data.truncated && (
						<p className="text-muted-foreground text-xs" data-truncated>
							showing {listing.data.entries.length} of {listing.data.total} entries
						</p>
					)}
				</section>
			)}

			{open !== null && (
				<section data-file={open} className="grid gap-2 rounded-md border p-3">
					<header className="flex flex-wrap items-center gap-2">
						<h3 className="min-w-0 flex-1 truncate font-medium text-xs">{open}</h3>
						<Toggle
							type="button"
							size="sm"
							variant="outline"
							pressed={raw}
							onPressedChange={setRaw}
						>
							Raw
						</Toggle>
						<Button
							type="button"
							variant="ghost"
							size="icon"
							aria-label="Close file"
							onClick={() => setOpen(null)}
						>
							<X aria-hidden />
						</Button>
					</header>
					{file.isPending ? (
						<Skeleton className="h-8 w-full" />
					) : file.isError ? (
						<Refusal error={file.error} />
					) : file.data.kind === "text" ? (
						<pre className="max-h-96 overflow-auto rounded-md border p-2 text-xs">
							{file.data.text}
						</pre>
					) : (
						<p data-binary className="text-muted-foreground text-xs">
							binary content · {fileSize(file.data.bytes)}
						</p>
					)}
				</section>
			)}
		</div>
	);
}
