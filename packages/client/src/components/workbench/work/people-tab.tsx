import type { TranscriptSnapshot } from "#/operator/transcript";
import { joinedParticipants } from "#/operator/work-view";

export function PeopleTab({ transcript }: { transcript: TranscriptSnapshot }) {
	const joined = joinedParticipants(transcript.entries);
	const presence = transcript.presence;

	return (
		<div className="grid gap-4 p-4 text-sm">
			<section data-joined className="grid gap-1">
				<h3 className="font-medium" id="people-joined">
					Joined
				</h3>
				<p className="text-muted-foreground text-xs">
					Everyone who has taken part in this Workspace.
				</p>
				{joined.length === 0 ? (
					<p className="text-muted-foreground text-xs">No one has joined yet.</p>
				) : (
					<ul className="grid gap-0.5" aria-labelledby="people-joined">
						{joined.map((participant) => (
							<li key={participant} data-participant={participant}>
								{participant}
							</li>
						))}
					</ul>
				)}
			</section>

			<section data-presence className="grid gap-1">
				<h3 className="font-medium" id="people-following">
					Following now
				</h3>
				<p className="text-muted-foreground text-xs">
					Everyone with this Workspace open right now.
				</p>
				{presence === undefined ? (
					<p className="text-muted-foreground text-xs">
						Who is following isn’t known until this tab connects to the Workspace.
					</p>
				) : presence.named.length === 0 && presence.anonymous === 0 ? (
					<p className="text-muted-foreground text-xs">No one is following right now.</p>
				) : (
					<ul className="grid gap-0.5" aria-labelledby="people-following">
						{presence.named.map((name) => (
							<li key={name} data-follower={name}>
								{name}
							</li>
						))}
						{presence.anonymous > 0 && <li data-anonymous>{presence.anonymous} anonymous</li>}
					</ul>
				)}
			</section>
		</div>
	);
}
