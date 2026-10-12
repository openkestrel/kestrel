import { Badge } from "#/components/ui/badge";
import type { Presence, Session, Workspace } from "#/operator/generated";
import { sessionPhase } from "#/operator/session-state";
import {
	continuityLine,
	followersLine,
	interruptingLabel,
	modelLine,
	sessionTitle,
} from "#/operator/session-view";

export function SessionHeader({
	read,
	session,
	presence,
	workspaces,
}: {
	read: Workspace;
	session: Session | undefined;
	presence: Presence | undefined;
	workspaces: Workspace[] | undefined;
}) {
	const followers = followersLine(presence);
	const continuity = continuityLine(read, workspaces);
	const interrupting = interruptingLabel(session);

	return (
		<header className="shrink-0 border-b px-4 py-2" data-session-header>
			<div className="flex flex-wrap items-center gap-2">
				<p className="truncate font-semibold text-sm" data-session-title>
					{sessionTitle(session, read)}
				</p>
				<Badge variant="secondary" data-session-state>
					{session ? sessionPhase(session) : "no session"}
				</Badge>
				{interrupting && (
					<span className="text-muted-foreground text-xs" data-session-interrupting>
						{interrupting}
					</span>
				)}
			</div>
			<dl className="mt-1 grid gap-x-4 gap-y-0.5 text-muted-foreground text-xs sm:grid-cols-2">
				<div className="flex gap-1">
					<dt className="sr-only">Model</dt>
					<dd data-session-model>{modelLine(session)}</dd>
				</div>
				{continuity && (
					<div className="flex gap-1">
						<dt className="sr-only">Continuity</dt>
						<dd data-session-continuity>{continuity}</dd>
					</div>
				)}

				{followers && (
					<div className="flex gap-1">
						<dt className="sr-only">Followers</dt>
						<dd data-session-followers>{followers}</dd>
					</div>
				)}
			</dl>
		</header>
	);
}
