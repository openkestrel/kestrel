# Sign-ins

What a saved sign-in is, how a Session is told to use one, how one is tested, and how a
subscription login is relayed. The Operator, readiness and the harness catalogue are in
[Operator setup](operator-setup.md).

## Saved sign-ins

`PUT …/sign-ins/{harness}/{method}` takes `{value}` and holds it where the catalogued method says,
asking nobody (`sign_in.rs`): a subscription in the Operator's own Profile, declared under their
name when they have none, and a key as the Organization's Provider Credential. Several
Operator-owned Profiles with none alone holding the harness's subscription are a 409, never a
choice. Shape is refused 422 before storage is touched, and a refusal never repeats the value.

Only an Anthropic or OpenAI key is checked, by one authenticated model-list read bounded to ten
seconds (`sign_in/check.rs`); nothing spends model usage. Only a 401 rejects (422
`credential_rejected`, keeping what was saved before); a refusal about permission or credit
saves the key unchecked; a provider that cannot answer or limits the rate (408, 429, 5xx, a
timeout) saves it unchecked and answers 503 `unavailable` whose `context.saved` is what was saved. The slot's revision is read before the
check and compared at commit, so material replaced meanwhile is a 409 and stands.

Every write of a Provider Credential or Profile entry, through these routes or the generic ones
or a Session's refresh, mints a new `revision`; generic writes are never checked and read as
`unchecked`. `GET …/sign-ins` lists each catalogued method with material, a shared key once per
harness offering it, with its revision, `authentication` evidence and `model_use` evidence for
that harness. The ten-second deadline and provider addresses are injected through
`Listening::checking_sign_ins_with`, which the test support points at a stub.

## Selecting a sign-in and what a Session used

Opening a Workspace and enqueueing a Session take an optional `sign_in_method`
(`sign_in/handoff.rs`). It must be a method the Session's harness offers whose material is saved
where the work will look: the Organization for a key, the Workspace's Profile for a
subscription. A method of another harness, or a subscription with no Profile named and none of
the Operator's holding it, is a 422; one with nothing saved is a 409 `sign_in_not_saved`. The
Operator's Profile never stands in for a Profile the Workspace names. A Session enqueued without one continues with the latest Session's
while the harness is the same, so only naming a method switches credentials.

A Session read carries `sign_in_method` and `supplied`: each variable or file its harness was
last spawned with, by revision and never by value, with whether that revision is still held.
`refreshed_from` on a saved sign-in is the revision a harness's own refresh advanced.

An `authentication_failed` diagnostic names the sign-in in `context.attribution`:

| Attribution | When | Effect |
| --- | --- | --- |
| `established` | A method was selected and handed over from where it is held, or one catalogued sign-in was all the harness was handed | The context carries the method, where it is held, the revision and whether it is `current`. Only a current revision's `authentication` evidence becomes `authentication_failed`, from source `session`. |
| `uncertain` | No method was selected and the harness was handed more than one variable or file, catalogued or not | Nothing is marked; `candidates` lists the catalogued ones and the first step is inspection. |
| `unattributed` | It was handed none kestrel catalogues | Nothing is marked. |

No evidence a supervisor carries establishes expiry or coverage, so `expired` and `covered` stay
null. A Turn answered on an established current revision records `model_use` `worked` for the
model it ran on; a refusal records `authentication_failed` for that model alone, and leaves the
revision's `provider_check` as it was. Neither rewrites another model's result.

## Testing a saved sign-in

`POST …/sign-ins/{harness}/{method}/tests` takes an optional `{model}` and answers 202 with a
test in `pending` (`sign_in/model_test.rs`); `GET …/tests/{test}` reads it, 200 whatever it
found. Saving never starts one, and nothing else spends model usage on a sign-in.

- **Before it is accepted.** The method must be catalogued and its material saved, a
  subscription in exactly one of the Operator's own Profiles (409 `sign_in_not_saved`, or
  `several_operator_profiles`). A method with `test_providers` needs a compatible
  `provider/model` (422); any other tests the model named, or the harness's default. The image
  is inspected then, never pulled: `Images::read` gates as `admit` does, and its identity is
  kept on the test. Profile-held material takes a `credential_use` hold in the transaction that
  writes the test, so a login in use is a 409 `credential_in_use` and no test waits behind it.
- **Running.** The work role claims a pending test in its dispatch loop and gives it two
  minutes. It reads the one entry the method fills, refusing `material_replaced` when the
  revision is no longer the one selected, provisions an Instance named
  `kestrel-sign-in-test-<id>` and starts the supervisor in
  [probe mode](link.md#harness-side). That Instance has no checkout, no link and no
  Workspace; its harness is handed that entry alone, plus the variable that enables only the
  named provider (`catalogue::confining`). `model` is recorded when the probe says it is
  ready, before the prompt. The probe gives up at the deadline itself, so a login it refreshed
  by then is still handed back; one that says nothing five seconds later is stopped.
- **Ending** is one transaction: a login file the harness rewrote is kept by compare-and-set
  against the tested revision, evidence is recorded, the hold is released and the test ends.

  | Outcome | State | Recorded |
  | --- | --- | --- |
  | `worked` | `completed` | `model_use` `worked` for the harness and model, from `model_test`, with the image |
  | `authentication_failed` | `failed` | The revision's `authentication`, from `model_test`; `model_use` when the model is known |
  | `not_covered` | `failed` | `model_use` `not_covered` for that model alone: the harness did not offer it, and no prompt was sent |
  | `unavailable` | `failed` | Nothing: the deadline, an Instance that could not start, or a work role with nowhere to run one |
  | `harness_failed`, `material_replaced` | `failed` | Nothing |

  Evidence goes to the revision a kept refresh minted, otherwise to the one tested. A test
  that records nothing leaves earlier evidence and its `observed_at` as they were.
- **Stopping.** The Instance is destroyed however the test ends. A control plane that stops
  ends its running tests `interrupted`, and a login refreshed in one of those is lost with it; one that starts ends every unfinished test
  `interrupted` and destroys the Instance its name gives, beside clearing every
  `credential_use`. Nothing resumes a test, since that would send a second prompt.

## Subscription sign-in relays

`POST …/sign-ins/{harness}/{method}/relay` starts a Claude `setup-token` or Codex device-authorization relay,
after the same image capability gate as a model test; optional `{profile}` selects one of the Operator's
own Profiles, otherwise the save path resolves or declares theirs. A relay has a non-secret UUID,
its exact destination, a monotonically increasing `version`, and an absolute ten-minute `deadline`.
A partial unique index and a write transaction enforce one active relay per Profile entry, across
harnesses and methods; another start answers 409 with its current locator and typed continue/cancel
steps. Distinct Profiles and Organizations proceed independently.

`GET …/relay/{relay}` returns the latest snapshot. `POST …/code {code}` accepts one hidden answer
only in `awaiting_code`, commits `saving` and hands the answer through a bounded in-memory mailbox;
it never persists the answer. Inspect a known relay after losing any write response. `DELETE`
ends an active relay as `cancelled` and returns an existing terminal result unchanged. Every route
checks the Organization/harness/method scope before exposing state. Terminal snapshots remain
readable across restart for 24 hours after finishing, then answer 410; old snapshots are cleared,
leaving scoped tombstones.

The work role claims attempts and runs the pinned image's `claude setup-token` in a fresh Instance,
through a 240-column, 40-row PTY (`sign_in/relay.rs`, `compute/terminal.rs`). The bounded VT screen
decoder reconstructs wrapped first-party authorization URLs, recognizes OAuth failure screens,
intercepts the token privately and supplies continuation Enter when needed. Saving requires both
the captured token and a natural successful exit. No raw screen, pasted answer or token enters
relay metadata, responses, events or logs, and no inference is performed. Completion records
`login_completed` authentication evidence and no model-use evidence.

A Codex relay runs `codex login --device-auth` with file credential storage instead. Its decoder
publishes the first-party verification page and one-time code as `awaiting_browser`; the vendor
polls for approval itself, so `POST …/code` answers 409 in that state. After a successful exit the
relay commits `saving`, reads `.codex/auth.json` from the Instance's home, and saves it only if it
is a ChatGPT login file. A prompt it cannot read within a minute, a missing, oversized or
incompatible file, or a failed vendor exit fails with a fixed message and a `sign_in` step to
import the file instead; vendor output never reaches the diagnostic.

An accepted Profile entry write cancels its active relay in the same transaction. Relay completion
claims its terminal outcome and compares the original material revision before holding anything,
so an import or cancellation wins over a late result. Cleanup belongs to a claimed worker until it
releases the attempt; the maintenance pass retries unattended cleanup. A restart first marks
unfinished attempts failed, then cleans up their deterministically named Instances. Local PTY
processes keep the same inherited cleanup locks as supervisors. Provisioning is cancellable and bounded by the relay deadline; each Docker removal request has
a ten-second budget. Daemon cleanup failure is retried
without preventing the control plane from serving retained state.

The browser polls. CLI consumers may reconnect to `GET …/events`: it sends the latest `relay`
snapshot immediately, then changed versions, and a terminal snapshot followed by `end` before
closing. Dropping either read or event connections leaves the relay running to its deadline.
