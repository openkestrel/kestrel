# Operator setup and sign-ins

The routes that take an install from nothing to a first Session: the Operator, readiness, defaults,
the harness catalogue, saved sign-ins and image capabilities. What every route shares is in
[Operator boundary](operator-boundary.md).

## The Operator

`GET /operator/operator` reads the install's Operator, or answers 404 with a `setup_gap` and
`name_operator` action. `PUT /operator/operator {name}` creates or renames that record without
changing its ID. First creation declares an Organization with that name only when none exists,
in the same transaction; naming does not complete initial setup.

`GET /operator/install` answers `{ initial_setup_complete }` whether or not an Operator is named.
It is a row in `install`, false on a new install and independent of readiness and of what is
declared. Nothing sets it yet: naming, a saved sign-in, an open and an enqueue never do, and the
shared start will with its first committed Workspace and Session.

## Readiness

`GET /operator/readiness` answers 200 whatever it finds (`start_readiness.rs`,
[ADR-0052](../adr/0052-a-refusal-carries-its-next-steps.md)): `outcome` is `ready`, `blocked` or
`unavailable`, beside the Organization read, the configured Client URL, `initial_setup_complete`,
the `selected` path, its one `sign_in` when exactly one is established, and what is already
`saved` (the Operator, the saved sign-ins, the GitHub Integrations that lend authority now, the
Projects). It reads Store and inspects the image, and nothing else: no provider is asked, no
relay started, nothing written, no image pulled.

- **Scope.** `organization` is the one named or the only one; several and none named is a 404
  `ambiguous_reference`, and one that does not exist a 404. `harness`, `model`, `profile`,
  `sign_in_method` and `project` narrow the path; a method no harness offers is a 422. A named
  Profile or Project that is not declared is a gap, the producer's own `missing_reference`.
- **Any path, or the chosen one.** With no harness named every saved sign-in of a harness the
  image carries is an alternative, so one usable path is ready and broken material elsewhere is
  a warning. With a harness named and no method the path is what an open naming none takes: the
  Operator's own Profile's subscription with the Organization's key withheld, and several such
  Profiles are the `several_operator_profiles` conflict rather than a choice. `selected` names a
  value only where it was given or exactly one remains.
- **Evidence.** A revision whose `authentication` is `authentication_failed`, `expired` or
  `not_covered` needs attention; `unchecked` is usable, and warns when every usable sign-in is
  unchecked. With a `model` named, that model's `model_use` on the revision counts too, and no
  other model's does. A model never tested is no prerequisite.
- **`gaps`** are Diagnostics in repair order, every one that could be established: the Operator
  (`operator`); no saved sign-in on a carried harness (`sign_in`), or the named harness the
  image does not carry (`harness_in_image`, with no sign-in gap beside it); saved sign-ins that all need attention
  (`sign_in_attention`); no Project and no lending GitHub Integration (`repository`, offering
  `connect_github` and then `declare_project`, since a public repository needs no Integration);
  no Project (`project`). `warnings` hold what does not block, a disabled Integration that
  would otherwise reach a repository among them. A harness outside the catalogue has no guided
  sign-in to find saved or missing, so naming one raises no sign-in gap.
- **`unavailable`** holds a failed image inspection and is never a gap: the harness is neither
  absent nor present. The outcome is `blocked` when a gap is known, `unavailable` when none is
  but an inspection failed.

## Operator defaults

What a write from a Client leaves unnamed is the Operator's
([ADR-0049](../adr/0049-an-install-has-one-operator-and-by-default-one-organization.md)):

- **Participant.** A post, a Held Message edit or withdrawal, an interrupt, an option change and
  an Instance release with no `participant` are written under the Operator's current name
  (`participant::named_or_operator`); with none named they are a 409 `setup_gap` on `participant`.
  An open's or a start's Brief is the Operator's when one is named and unsigned otherwise. A name
  given is never replaced, and a rename changes only what is written next.
- **Subscription Profile.** An open, a start or an enqueue naming no Profile takes the Operator's
  own Profile that holds a subscription the catalogue offers the Session's harness
  (`sign_in::handoff::defaulted`), and selects that subscription as the Session's
  `sign_in_method`, so the Organization's key for the same harness is withheld and never answers a
  refused subscription. A Profile holding several of the harness's subscriptions is taken with no
  method selected. Several such Profiles are a 409 `several_operator_profiles`. No such Profile
  leaves the Organization's Provider Credentials, as does naming an Organization-held method. A
  Profile with a free-text owner is never chosen, whatever the text.
- An enqueue, and a person's post that starts the next Session, apply this only to a Workspace
  that names no Profile and that no Event opened (`work::next_as_the_operator`), and the
  Workspace then keeps the Profile it took. A Session continuing the latest one's method keeps
  it. A Trigger's Sessions, and a Session an Event's post starts, never take it.
- The Operator's name is held to the participant rule's shape when it is given, so it can always
  be written; an Agent of the same name in an Organization still refuses the default there.

Profile declaration defaults an omitted `owner` to the current Operator, or refuses 409 with the
same setup gap. Reads expose `owner_operator` beside `owner`; Operator-owned Profiles show the
current Operator label and keep their identity and material on rename. An explicit free-text
owner stays generic even when its text matches the Operator's label.

## Harness catalogue

`GET /operator/harnesses` reads the product harnesses, default ACP commands and Sign-in Methods.
The data in `crates/kestrel/src/catalogue/harnesses.json` supplies both the operator metadata and
unconfigured dispatch commands; explicit harness-command configuration remains authoritative.
Subscriptions name Operator ownership and keys Organization ownership. Each method names its
accepted token/file input, a typed variable/file fill target, and any relay or console link.
`test_providers` is on a method of a harness that routes by provider: a model test of it names
its model as `provider/model`, the provider one of those listed.
Metadata needs no credentials. Each row's `availability` comes from inspecting the configured
image's `dev.kestrel.harnesses` label at that read (ADR-0048): `available` or `not_carried` after a
successful inspection, `unavailable` with an `unavailable` diagnostic when the image is missing or
the daemon fails, and `unchecked` when the compute driver runs no image. Label names outside the
catalogue add no rows.
`GET /operator/harnesses/{harness}/sign-in-methods/{method}` reads one supported combination;
unknown harnesses or methods answer a typed `invalid_field` with supported choices. Custom
harness commands and generic Profiles remain usable without guided methods (ADR-0046).

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
  ready, before the prompt.
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
  ends its running tests `interrupted`; one that starts ends every unfinished test
  `interrupted` and destroys the Instance its name gives, beside clearing every
  `credential_use`. Nothing resumes a test, since that would send a second prompt.

## Image capabilities

`capability.rs` resolves the configured image reference again on every read, through
`docker image inspect` and never a pull, and caches only parsed labels, keyed by the image ID the
reference resolved to. A failed inspection is `Unavailable`; it never falls back to an earlier
positive result. `Images::admit` gates declaring an Agent, previewing or applying a declaration,
and a start. It runs before any transaction opens, so a refusal writes nothing. A harness the
inspected image does not declare is a 409 `setup_gap` (`harness_in_image`) naming the image and
harness. An uninspectable image is a 503 `unavailable` (`image_inspection`) whose steps inspect
the image and then retry the capability read. Label membership admits a harness and leaves its
configured command alone; a command still missing at spawn fails the Session with
`executable_missing` evidence. The serve role logs one inspection at startup and serves either
way. `Images::read` is the one capability read for any later consumer.
