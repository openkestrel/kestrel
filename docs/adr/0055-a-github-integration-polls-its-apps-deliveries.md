# A GitHub Integration polls its App's Deliveries, and listens where GitHub can reach it

A GitHub Integration was either polled or signed, fixed at registration, and only a signed one ever
learned a pull request (#506). Yet the default install is a localhost control plane that GitHub
cannot reach. We decided that every GitHub Integration's App hook is active, and that its poll reads
the App's own log of Deliveries (`GET /app/hook/deliveries`) rather than the repository. GitHub
records a Delivery whether or not it reached kestrel, and keeps its payload. A Delivery that does
reach kestrel's webhook is the same Delivery, with the same GUID. Polling is the floor, a reachable
webhook only adds speed, and both run at once, because the Delivery's GUID is the Event's `id`
either way. Nothing switches between them, so nothing falls between them or is recorded twice.

## Considered Options

- **A webhook is required for pull requests.** Rejected: the default install would never learn
  one, and GitHub never redelivers on its own.
- **Poll the repository's resources, and switch to the webhook where it works.** Rejected because
  the two sources name the same event differently. A webhook `issues.labeled` carries no
  issue-event id, and a polled pull request is a snapshot of its state, not the change. Running both
  records a label twice and fires its Triggers twice. Switching between them loses whatever falls
  in the gap at each switch, and inferring pull request transitions from snapshots collapses pushes
  and misses a close that was followed by a reopen.
- **`gh webhook forward`** as the easy path to a webhook. Rejected: GitHub supports it for testing
  only. It allows one forwarder per repository, runs on a person's token rather than the App's
  ([ADR-0028](0028-an-integration-lends-a-run-its-identity.md)), and drops everything while it is
  not running.

## Consequences

- **A polled Event is the webhook's Event.** Its type and payload are the Delivery's, so
  [ADR-0011](0011-cloudevents-is-the-events-shape.md)'s promise that a Trigger survives the move
  between polling and a webhook holds for every type the App subscribes to.
- **A hook nobody can reach is still active.** Its URL is the control plane's own webhook path
  under `--webhook-base` when there is one, otherwise under `https://unreachable.invalid`. GitHub
  refuses a loopback or `localhost` hook URL, but accepts a public name without resolving it.
  Making the hook reachable later changes only its URL.
- **GitHub's 3-day retention is the horizon.** Deliveries older than that are gone. The Integration
  says so, and kestrel never rebuilds the missing Events from a resource read.
- **This rests on GitHub behaviour the documentation does not state.** The #506 spike found that an
  App hook stayed active through a quarter hour of failed Deliveries (longer is unproven), that
  GitHub neither limits nor reports a limit on requests made as the App at polling rates, and that
  a Delivery is listed within seconds, newest first, under the GUID a webhook carries. If that
  changes, resource polling plus asking GitHub to redeliver what failed is the fallback, and this
  ADR is revised.
