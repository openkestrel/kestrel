import { Diagnosis } from "#/components/refusal";
import { Webhook } from "#/components/settings/github";
import { IntegrationMaintenance } from "#/components/settings/integration-maintenance";
import { Badge } from "#/components/ui/badge";
import type { Integration, WebhookSecretRotation } from "#/operator/generated";
import { lifecycleOf } from "#/operator/integrations";

function at(time: string): string {
	return new Date(time).toLocaleString();
}

export function IntegrationCard({
	organization,
	integration,
	retry,
}: {
	organization: string;
	integration: Integration;
	retry: () => void;
}) {
	const lifecycle = lifecycleOf(integration, organization);
	return (
		<li
			data-integration={integration.name}
			data-integration-state={integration.state}
			className="grid gap-2 border p-3 text-sm"
		>
			<div className="flex flex-wrap items-center gap-x-3 gap-y-1">
				<span className="font-medium">{integration.name}</span>
				<Badge
					variant={
						lifecycle.state === "enabled"
							? "secondary"
							: lifecycle.state === "retired"
								? "outline"
								: "destructive"
					}
					data-lifecycle-state
				>
					{lifecycle.label}
				</Badge>
				{lifecycle.since && (
					<span className="text-muted-foreground text-xs">since {at(lifecycle.since)}</span>
				)}
				<span className="text-muted-foreground text-xs">
					watches {integration.repository ?? "no repository"}
				</span>
			</div>
			<p className="text-muted-foreground text-xs" data-lifecycle>
				{lifecycle.detail}
			</p>
			{integration.diagnostic && <Diagnosis diagnostic={integration.diagnostic} retry={retry} />}
			{integration.webhook_secret_rotation && (
				<Rotation rotation={integration.webhook_secret_rotation} retry={retry} />
			)}
			{integration.state === "retired" ? (
				<History integration={integration} />
			) : (
				<Webhook organization={organization} integration={integration} />
			)}
			<IntegrationMaintenance organization={organization} integration={integration} retry={retry} />
		</li>
	);
}

function Rotation({ rotation, retry }: { rotation: WebhookSecretRotation; retry: () => void }) {
	return (
		<section
			aria-label="Unfinished webhook-secret rotation"
			className="grid gap-1 text-xs"
			data-rotation
		>
			<p>
				A webhook-secret rotation started {at(rotation.started_at)} is unfinished. Deliveries signed
				with either secret are accepted until GitHub acknowledges the new one.
			</p>
			<p className="text-muted-foreground">
				{rotation.attempted_at === null
					? "GitHub has not been asked to set it yet."
					: `GitHub was last asked to set it at ${at(rotation.attempted_at)}.`}
			</p>
			<Diagnosis diagnostic={rotation.diagnostic} retry={retry} />
		</section>
	);
}

function History({ integration }: { integration: Integration }) {
	return (
		<section
			aria-label={`${integration.name}'s history`}
			className="grid gap-1 text-xs"
			data-history
		>
			<p className="text-muted-foreground">
				Its Events, and the Projects, Triggers and Workspaces that name it, stay readable.
			</p>
			{integration.canceled_posts.length === 0 ? (
				<p>Retiring it left nothing unposted.</p>
			) : (
				<>
					<p>Retiring it left these unposted:</p>
					<ul className="grid gap-0.5">
						{integration.canceled_posts.map((post) => (
							<li key={`${post.session}:${post.turn ?? "outcome"}`} data-canceled-post>
								<span className="font-mono">{post.session}</span>
								{post.turn === null ? "'s Outcome" : `'s Turn ${post.turn}`}, canceled{" "}
								{at(post.canceled_at)}: {post.reason}
								{post.attempted_at !== null &&
									`. A request had already gone out at ${at(post.attempted_at)}, so the comment may be on GitHub all the same.`}
							</li>
						))}
					</ul>
				</>
			)}
		</section>
	);
}
