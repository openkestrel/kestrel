import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { useState } from "react";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { Input } from "#/components/ui/input";
import { Skeleton } from "#/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "#/components/ui/toggle-group";
import { ScrollablePre } from "#/components/workbench/scrollable-pre";
import { cn } from "#/lib/utils";
import { changesQuery, commitsQuery, stashesQuery } from "#/operator/work-queries";
import { DIFF_SCOPES, repositoryName, scopeLabel } from "#/operator/work-view";

export function DiffTab({ organization, workspace }: { organization: string; workspace: string }) {
	const [scope, setScope] = useState<string>("unpublished");
	const [revision, setRevision] = useState("");
	const [asked, setAsked] = useState("");
	const applied = scope === "commit" ? `commit:${asked}` : scope;
	const changes = useQuery({
		...changesQuery(organization, workspace, applied),
		enabled: scope !== "commit" || asked !== "",
	});

	return (
		<div className="grid gap-4 p-4 text-sm">
			<div className="flex flex-wrap items-center gap-2">
				<ToggleGroup
					aria-label="Diff scope"
					size="sm"
					value={[scope]}
					onValueChange={(values) => {
						const next = values.at(-1);
						if (next) setScope(next);
					}}
				>
					{DIFF_SCOPES.map((value) => (
						<ToggleGroupItem key={value} value={value}>
							{scopeLabel(value)}
						</ToggleGroupItem>
					))}
					<ToggleGroupItem value="commit">Commit</ToggleGroupItem>
				</ToggleGroup>
				{scope === "commit" && (
					<form
						className="flex items-center gap-2"
						onSubmit={(event) => {
							event.preventDefault();
							setAsked(revision.trim());
						}}
					>
						<Input
							aria-label="Commit revision"
							placeholder="revision"
							value={revision}
							onChange={(event) => setRevision(event.target.value)}
						/>
						<Button type="submit" size="sm" variant="outline">
							Read commit
						</Button>
					</form>
				)}
			</div>

			{changes.isPending ? (
				asked === "" && scope === "commit" ? (
					<p className="text-muted-foreground">Name a revision to read its diff.</p>
				) : (
					<Skeleton className="h-8 w-full" />
				)
			) : changes.isError ? (
				<Refusal error={changes.error} />
			) : (
				changes.data.repositories.map((repository) => (
					<article
						key={repository.repository}
						data-diff={repository.repository}
						className="grid gap-2 rounded-md border p-3"
					>
						<header className="flex flex-wrap items-baseline gap-2">
							<h3 className="font-medium">{repositoryName(repository)}</h3>
							<span className="text-muted-foreground text-xs">
								{scopeLabel(applied)}
								{repository.files.length > 0 &&
									` · ${repository.files.length} ${repository.files.length === 1 ? "file" : "files"}`}
							</span>
							{repository.truncated && (
								<span data-truncated className="text-destructive text-xs">
									truncated
								</span>
							)}
						</header>
						{repository.files.length > 0 && (
							<ul className="grid gap-0.5 text-xs">
								{repository.files.map((file) => (
									<li key={file.path} className="flex flex-wrap gap-x-2">
										<span className="truncate">{file.path}</span>
										{file.added !== null && (
											<span className="text-muted-foreground">
												+{file.added} −{file.removed ?? 0}
											</span>
										)}
									</li>
								))}
							</ul>
						)}
						<ScrollablePre className="max-h-96 overflow-auto rounded-md border p-2 text-xs">
							{repository.diff === "" ? "no changes" : repository.diff}
						</ScrollablePre>
					</article>
				))
			)}

			<TextSection
				title="Commits"
				organization={organization}
				workspace={workspace}
				kind="commits"
			/>
			<TextSection
				title="Stashes"
				organization={organization}
				workspace={workspace}
				kind="stashes"
			/>
		</div>
	);
}

function TextSection({
	title,
	organization,
	workspace,
	kind,
}: {
	title: string;
	organization: string;
	workspace: string;
	kind: "commits" | "stashes";
}) {
	const [open, setOpen] = useState(false);
	const query = kind === "commits" ? commitsQuery : stashesQuery;
	const read = useQuery({ ...query(organization, workspace), enabled: open });

	return (
		<Collapsible open={open} onOpenChange={setOpen}>
			<div className="rounded-md border">
				<CollapsibleTrigger className="w-full">
					<span className="flex items-center gap-2 px-3 py-2 text-left">
						<ChevronRight
							aria-hidden
							className={cn("size-4 transition-transform", open && "rotate-90")}
						/>
						<span className="font-medium">{title}</span>
					</span>
				</CollapsibleTrigger>
				<CollapsibleContent>
					<div className="border-t p-3">
						{read.isPending ? (
							<Skeleton className="h-6 w-full" />
						) : read.isError ? (
							<Refusal error={read.error} />
						) : (
							read.data.repositories.map((repository) => (
								<div key={repository.repository} className="grid gap-1">
									<span className="text-muted-foreground text-xs">
										{repositoryName(repository)}
									</span>
									<pre className="max-h-64 overflow-auto rounded-md border p-2 text-xs">
										{repository.text === "" ? `no ${title.toLowerCase()}` : repository.text}
									</pre>
								</div>
							))
						)}
					</div>
				</CollapsibleContent>
			</div>
		</Collapsible>
	);
}
