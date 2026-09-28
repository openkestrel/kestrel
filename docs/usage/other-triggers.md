# Webhooks and schedules

Generic webhooks and schedules use the same event, trigger, and workspace path as GitHub. See [GitHub automation](github-automation.md#declare-a-trigger) for trigger filters, templates, testing, and firing limits.

## Receive a generic webhook

Register an integration with a shared secret:

```sh
kestrel integration register webhook ci --secret "$KESTREL_WEBHOOK_SECRET"
kestrel integration list
```

The list shows its `/webhooks/<integration id>` path. A sender must POST to that path with `Authorization: Bearer <secret>`. Kestrel accepts CloudEvents in binary mode (`ce-*` headers and a data body) or structured mode (`application/cloudevents+json`). Other POST bodies become events of type `dev.kestrel.webhook.received`. Duplicate events with the same `source` and `id` are recorded once.

The Compose stack exposes the operator API on port 7718, but does not publish the webhook listener on port 7717. To receive local test requests, add this to a gitignored `compose.override.yaml` and restart the stack:

```yaml
services:
  kestrel:
    ports:
      - "127.0.0.1:7717:7717"
```

Apply the override with `docker compose up -d`. Then send an event, using the integration ID from `kestrel integration list`:

```sh
curl -X POST "http://127.0.0.1:7717/webhooks/$INTEGRATION" \
  -H "Authorization: Bearer $KESTREL_WEBHOOK_SECRET" \
  -H 'ce-specversion: 1.0' -H 'ce-id: build-7' \
  -H 'ce-source: https://ci.example.com/pipelines/3' \
  -H 'ce-type: com.example.build.failed' \
  -H 'Content-Type: application/json' -d '{"step": "test"}'
```

Kestrel responds `202 Accepted` after recording the event. `kestrel event list` and `kestrel event show <record>` let you inspect it before writing a trigger. For an external sender, route the webhook listener through an ingress that can reach the control plane.

## Run work on a schedule

A scheduled trigger declares an interval or a cron expression in place of an event filter:

```sh
kestrel trigger declare sweep \
  --every 24h \
  --brief 'Sweep the backlog for stale issues as of {{ event.time }}' \
  --project kestrel \
  --agent builder
```

Kestrel records a `dev.kestrel.schedule.elapsed` event at each due time and fires the trigger through the usual path. The brief can use the event's `time`, `id`, and schedule data. Missed times during downtime produce one firing when Kestrel resumes. Disabled triggers do not elapse. A schedule closer than six minutes between firings is refused because each trigger is limited to ten firings per hour. Scheduled work has no external issue to receive its outcome.

For work tied to a time of day, use a time zone and a five-field cron expression:

```sh
kestrel trigger declare triage \
  --cron '0 9 * * 1-5' \
  --zone America/New_York \
  --brief 'Triage what arrived since yesterday, as of {{ event.time }}' \
  --project kestrel \
  --agent builder
```

The fields are minute, hour, day of month, month, and day of week (`0` is Sunday). Numbers, `*`, ranges, lists, and steps are supported; names and `?`, `L`, or `W` are not. Restrict either day of month or day of week, but not both. The time zone is required. During a daylight saving change, a skipped time runs when the clock jumps forward and a repeated time runs on its first occurrence.

`kestrel trigger test triage` renders the next scheduled event without starting work and shows when it would run. `kestrel trigger show triage` reports the expression and zone.
