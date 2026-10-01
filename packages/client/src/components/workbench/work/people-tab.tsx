import { useTranscript } from "#/operator/currency";
import type { Currency } from "#/operator/currency";
import { joinedParticipants, presenceOf } from "#/operator/work-view";

export function PeopleTab({
	currency,
	organization,
	workspace,
}: {
	currency: Currency;
	organization: string;
	workspace: string;
}) {
	const transcript = useTranscript(currency, organization, workspace);
	const joined = joinedParticipants(transcript.entries);
	const presence = presenceOf(transcript.presence);

	return (
		<div className="grid gap-4 p-4 text-sm">
			<section data-joined className="grid gap-1">
				<h3 className="font-medium">Joined</h3>
				{joined.length === 0 ? (
					<p className="text-muted-foreground text-xs">No Participant has joined yet.</p>
				) : (
					<ul className="grid gap-0.5">
						{joined.map((participant) => (
							<li key={participant} data-participant={participant}>
								{participant}
							</li>
						))}
					</ul>
				)}
				<p className="text-muted-foreground text-xs">
					Joined Participants are durable Workspace state.
				</p>
			</section>

			<section data-presence className="grid gap-1">
				<h3 className="font-medium">Following now</h3>
				{presence === undefined ? (
					<p className="text-muted-foreground text-xs">
						Presence is unknown until this tab follows the Workspace.
					</p>
				) : presence.named.length === 0 && presence.anonymous === 0 ? (
					<p className="text-muted-foreground text-xs">No one is following.</p>
				) : (
					<ul className="grid gap-0.5">
						{presence.named.map((name) => (
							<li key={name} data-follower={name}>
								{name}
							</li>
						))}
						{presence.anonymous > 0 && <li data-anonymous>{presence.anonymous} anonymous</li>}
					</ul>
				)}
				<p className="text-muted-foreground text-xs">
					Presence is transient; it never enters the Transcript.
				</p>
			</section>
		</div>
	);
}
