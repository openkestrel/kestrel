import { useQuery } from "@tanstack/react-query";
import { ChevronRight } from "lucide-react";
import { useState } from "react";
import { CodeBlock } from "#/components/ai-elements/code-block";
import { Refusal } from "#/components/refusal";
import { Button } from "#/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "#/components/ui/collapsible";
import { Input } from "#/components/ui/input";
import { Skeleton } from "#/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "#/components/ui/toggle-group";
import { cn } from "#/lib/utils";
import type { RepositoryDiff } from "#/operator/generated";
import { changesQuery, commitsQuery, stashesQuery } from "#/operator/work-queries";
import {
	DIFF_SCOPES,
	filesTotals,
	repositoryName,
	scopeLabel,
	splitPath,
	statText,
} from "#/operator/work-view";

export function ChangesTab({
	organization,
	workspace,
}: {
	organization: string;
	workspace: string;
}) {
	const [scope, setScope] = useState<string>("unpublished");
	const [revision, setRevision] = useState("");
	const [asked, setAsked] = useState("");
	const [open, setOpen] = useState<string | null>(null);
	const applied = scope === "commit" ? `commit:${asked}` : scope;
	const changes = useQuery({
		...changesQuery(organization, workspace, applied),
		enabled: scope !== "commit" || asked !== "",
	});

	return (
		<div className="grid gap-4 p-4 text-sm" data-changes aria-busy={changes.isPending}>
			<div className="flex flex-wrap items-center gap-2">
				<ToggleGroup
					aria-label="Diff scope"
					size="sm"
					value={[scope]}
					onValueChange={(values) => {
						const next = values.at(-1);
						if (next) {
							setScope(next);
							setOpen(null);
						}
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
							setOpen(null);
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
				<Refusal error={changes.error} retry={() => void changes.refetch()} />
			) : changes.data.repositories.length === 0 ? (
				<p className="text-muted-foreground text-xs" data-no-changes>
					No repository to read.
				</p>
			) : (
				changes.data.repositories.map((repository) => (
					<RepositoryChanges
						key={repository.repository}
						repository={repository}
						scope={applied}
						open={open}
						onOpen={setOpen}
						organization={organization}
						workspace={workspace}
					/>
				))
			)}

			<Commits organization={organization} workspace={workspace} />
			<Stashes organization={organization} workspace={workspace} />
		</div>
	);
}

function RepositoryChanges({
	repository,
	scope,
	open,
	onOpen,
	organization,
	workspace,
}: {
	repository: RepositoryDiff;
	scope: string;
	open: string | null;
	onOpen: (path: string | null) => void;
	organization: string;
	workspace: string;
}) {
	const totals = filesTotals(repository.files);

	return (
		<article data-diff={repository.repository} className="grid gap-1">
			<header className="flex flex-wrap items-baseline gap-2">
				<h3 className="font-medium">{repositoryName(repository)}</h3>
				<span className="text-muted-foreground text-xs">
					{scopeLabel(scope)}
					{repository.files.length > 0 &&
						` · ${repository.files.length} ${repository.files.length === 1 ? "file" : "files"}`}
				</span>
				<span className="text-muted-foreground text-xs" data-diff-total>
					{statText(totals.added, totals.removed)}
				</span>
				{repository.truncated && (
					<span data-truncated className="text-destructive text-xs">
						truncated
					</span>
				)}
			</header>
			{repository.files.length === 0 ? (
				<p className="text-muted-foreground text-xs">no changes</p>
			) : (
				<ul className="grid">
					{repository.files.map((file) => {
						const { folder, name } = splitPath(file.path);
						const expanded = open === file.path;
						return (
							<li key={file.path}>
								<button
									type="button"
									aria-expanded={expanded}
									className="flex w-full min-w-0 items-baseline gap-1 rounded-md px-1 py-1 text-left hover:bg-accent"
									onClick={() => onOpen(expanded ? null : file.path)}
								>
									<span className="min-w-0 truncate text-muted-foreground text-xs">{folder}</span>
									<span className="shrink-0 font-medium">{name}</span>
									<span
										className="ml-auto shrink-0 pl-2 text-muted-foreground text-xs"
										data-diff-stat
									>
										{statText(file.added, file.removed)}
									</span>
								</button>
								{expanded && (
									<div className="px-1 pb-2" data-file-diff={file.path}>
										<FileDiff
											organization={organization}
											workspace={workspace}
											scope={scope}
											path={file.path}
										/>
									</div>
								)}
							</li>
						);
					})}
				</ul>
			)}
		</article>
	);
}

// One file's slice of the diff, which the operator filters by the literal path.
function FileDiff({
	organization,
	workspace,
	scope,
	path,
}: {
	organization: string;
	workspace: string;
	scope: string;
	path: string;
}) {
	const diff = useQuery(changesQuery(organization, workspace, scope, [path]));

	if (diff.isPending) return <Skeleton className="h-16 w-full" />;
	if (diff.isError) return <Refusal error={diff.error} retry={() => void diff.refetch()} />;

	return (
		<>
			{diff.data.repositories.map((repository) => (
				<CodeBlock
					key={repository.repository}
					className="text-xs"
					code={repository.diff === "" ? "no changes" : repository.diff}
					language="diff"
				/>
			))}
		</>
	);
}

function Commits({ organization, workspace }: { organization: string; workspace: string }) {
	const read = useQuery(commitsQuery(organization, workspace));

	return (
		<section aria-label="Commits" className="grid gap-1">
			<h3 className="text-muted-foreground text-xs">Commits</h3>
			{read.isPending ? (
				<Skeleton className="h-6 w-full" />
			) : read.isError ? (
				<Refusal error={read.error} retry={() => void read.refetch()} />
			) : read.data.repositories.length === 0 ? (
				<p className="text-muted-foreground text-xs" data-no-commits>
					No repository to read.
				</p>
			) : (
				read.data.repositories.map((repository) => (
					<div key={repository.repository} className="grid gap-0.5">
						<span className="text-muted-foreground text-xs">{repositoryName(repository)}</span>
						<TextLines text={repository.text} empty="no commits" />
					</div>
				))
			)}
		</section>
	);
}

function Stashes({ organization, workspace }: { organization: string; workspace: string }) {
	const [open, setOpen] = useState(false);
	const read = useQuery({ ...stashesQuery(organization, workspace), enabled: open });

	return (
		<Collapsible open={open} onOpenChange={setOpen}>
			<div className="rounded-md border">
				<CollapsibleTrigger className="w-full">
					<span className="flex items-center gap-2 px-3 py-2 text-left">
						<ChevronRight
							aria-hidden
							className={cn("size-4 transition-transform", open && "rotate-90")}
						/>
						<span className="font-medium">Stashes</span>
					</span>
				</CollapsibleTrigger>
				<CollapsibleContent>
					<div className="border-t p-3">
						{read.isPending ? (
							<Skeleton className="h-6 w-full" />
						) : read.isError ? (
							<Refusal error={read.error} retry={() => void read.refetch()} />
						) : (
							read.data.repositories.map((repository) => (
								<div key={repository.repository} className="grid gap-1">
									<span className="text-muted-foreground text-xs">
										{repositoryName(repository)}
									</span>
									<TextLines text={repository.text} empty="no stashes" />
								</div>
							))
						)}
					</div>
				</CollapsibleContent>
			</div>
		</Collapsible>
	);
}

function TextLines({ text, empty }: { text: string; empty: string }) {
	const lines = text.split("\n").filter((line) => line.trim() !== "");
	if (lines.length === 0) {
		return <p className="text-muted-foreground text-xs">{empty}</p>;
	}
	return (
		<ul className="grid gap-0.5 font-mono text-xs">
			{lines.map((line) => (
				<li key={line} className="truncate">
					{line}
				</li>
			))}
		</ul>
	);
}
