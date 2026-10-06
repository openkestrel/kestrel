# An Integration keeps its identity through maintenance

An Integration's Organization, kind, GitHub API origin, repository, App and installation identify
the connection and cannot change in place. Its name, supported inbound/outbound directions,
polling interval and credentials can be maintained. Projects explicitly select an Integration for
each connected repository; a Workspace fixes that selection when it opens. Rebinding a Project
affects future Workspaces, never the authority of an existing one. This makes replacement
deliberate rather than letting removal silently select another connection.

## Lifecycle

Disable is reversible and pauses inbound and outbound use, preserving credentials, the inbound
cursor and pending outbound posts. Re-enable catches up within GitHub's Delivery retention window;
an unread gap is a visible refusal, never reconstructed history. Direction changes pause the
corresponding use with the same preservation rules. Unsupported directions and invalid polling
intervals are refused before saving.

Retire is permanent local removal from use: erase credentials and cached tokens, retain the readable
Integration and its historical references, and cancel pending outbound posts with a visible reason.
Neither operation deletes Projects, Triggers, Events, Workspaces or their history. A retired
Integration cannot be re-enabled or maintained. Retirement does not uninstall an App, delete it,
revoke its GitHub keys or modify a potentially shared installation; Clients offer optional cleanup
instructions. Neither disable nor retirement silently substitutes another Integration.

Queued Sessions and held Firings that need a disabled Integration remain paused and explain the
repair. After retirement they remain visibly blocked or are canceled according to their existing
state contract; no successful work is reported. Running Sessions continue, but subsequent requests
for GitHub credentials are refused. A credential already handed to a process or an HTTP request
already issued cannot be recalled; Clients must not promise remote revocation or cancellation.

## Rotation and concurrency

Validate a replacement App private key against the same App, installation and repository before
atomically replacing stored material. A failed check leaves the previous material in use. A
successful change invalidates installation-token caches and fences operations using the old
revision. Secrets remain sealed and are never returned in reads, errors, logs or Transcripts.

GitHub webhook-secret rotation is a durable, resumable operation rather than a purported atomic
change across two systems. Persist the sealed replacement and temporarily accept the old and new
secrets, update GitHub's hook secret, verify the change, then remove the old verifier. Failure or
restart exposes the unfinished operation and allows resumption; polling continues. Serialize
rotation with other maintenance, and refuse conflicting changes with a continuation repair.
Retirement can terminate the operation by erasing all its local secret material.

GitHub masks the secret in configuration reads. Confirmation is a successful authenticated
`PATCH /app/hook/config` acknowledgement, not equality with a read-back secret. If the response is
lost, resume by setting the same persisted replacement again before finalizing locally; do not
claim completion from the masked value
([GitHub's hook interface](https://docs.github.com/en/rest/apps/webhooks)).

The hook URL and secret belong to the App, not the repository. Refuse a webhook-secret or URL
change when another non-retired Integration uses the same App, including a disabled connection
and connections in other Organizations. Return a typed explanation without exposing another
Organization's records. Coordinated shared-App maintenance belongs to the deferred sharing design;
local disable, retirement and private-key replacement remain available.

An Integration revision guards maintenance, polling, webhook ingest, outbound posts and token
handoff. Recheck the current state and revision before committing fetched Events, advancing a cursor
or handing out a credential; stale work must not restore credentials, resume use or overwrite newer
diagnostics. Outbound requests already issued may have succeeded remotely: preserve that uncertainty
and reconcile by the existing post marker before retrying, rather than duplicating a comment.
Both Clients receive the same lifecycle state, typed refusals and next steps through the operator
interface; the Integration module owns these rules rather than each caller reconstructing them.

## Polling and ownership

[A polled GitHub Integration never learns a pull request](https://github.com/openkestrel/kestrel/issues/506)
owns always polling the App's Delivery log, the shared Delivery GUID on poll/webhook paths and the
retention-gap refusal. A reachable webhook adds speed; it does not replace polling or reset its
cursor. [Point a GitHub Integration's hook at a reachable URL](https://github.com/openkestrel/kestrel/issues/563)
keeps URL maintenance and reachability reporting. Lifecycle maintenance preserves those contracts
and neither forks their work nor reintroduces the old exclusive polling/webhook modes.

[An Integration cannot be changed or removed](https://github.com/openkestrel/kestrel/issues/507)
is the existing lifecycle implementation owner. The GitHub specification task sizes its build
slices and the browser/CLI consumers, while
[A Session reaches GitHub as its Project's Integration](https://github.com/openkestrel/kestrel/issues/260)
consumes explicit Project selection, the Workspace's fixed selection and lifecycle-aware per-use
credentials.

## Deferred scope

One repository per Integration remains the 0.4 model. GitHub itself permits an installation to
serve multiple repositories; this decision does not require one GitHub installation per repository.
Sharing installations, credential ownership across connections and routing across multiple
repositories remain explicit 0.7 work in
[Define the Integration domain and architecture](https://github.com/openkestrel/kestrel/issues/140).
The rung closes with deliberate maintenance of its existing single-repository connections.

## Considered alternatives

- Replacing repository or App identity in place changes the authority of historical and running
  work under an unchanged name.
- Deleting the Integration breaks historical references; reversible disable alone does not erase
  credentials when the operator permanently removes a connection.
- Repository-only lookup after retirement silently picks a replacement identity. Trusted Project
  selection and a Workspace's fixed selection keep the authority reviewed, as ADR-0013 requires.
- Treating webhook rotation as one transaction conceals the independent remote operation and leaves
  an interrupted change without a recoverable state.
