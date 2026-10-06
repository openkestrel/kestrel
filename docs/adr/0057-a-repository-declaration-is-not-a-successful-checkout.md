# A repository declaration is not a successful checkout

Repository declarations validate supported syntax before saving; they do not generally require
network access or imply a successful checkout. A declaration selecting a GitHub Integration is
the exception: before saving a new or changed repository binding or checkout base branch, verify
repository access and that branch through the selected App. This catches actionable setup mistakes
where kestrel already has authority to check them, without moving Git into the control plane or
pretending it can see an Instance's filesystem and network.

## Supported inputs

The shared declaration path accepts HTTP(S), `ssh://`, scp-style SSH, `git://`, `file://`, and local
paths beginning `/`, `./` or `../`. Local paths refer to the Instance's filesystem, never the
Client's or control plane's. Scheme/host/path structure and a usable checkout directory are checked
without contacting the source; empty values, bare names and unknown transports are refused.
Git remote-helper syntax and implicit transport aliases are not part of this support contract.

`owner/repo` and `github.com/owner/repo` are explicit GitHub conveniences, expanded to
`https://github.com/owner/repo.git` in the shared declaration path. A local path such as
`./owner/repo` stays local. The Clients show the resolved address before confirmation, and preview
and save use the same expansion. The public-repository setup path focuses on HTTPS, accepting the
same GitHub conveniences; full Project declarations retain the other supported forms.

The same rules cover Project declarations, declaration documents and first-start plans. Clients
may give early feedback, but the control plane owns the shared validation and resolved declaration.
Existing checkout-directory collision checks apply after resolving inputs. Reject syntactically
unusable repository values before any part of a declaration is saved, using the common typed
refusal contract and field context rather than Client-specific guesses.

## GitHub-backed declarations

A selected Integration must be usable under ADR-0056 and name the declared repository. Verify
repository access and the checkout base branch with that Integration's credentials. Authentication
failures, unavailable access, rate limits and outages refuse the save and preserve the previous
declaration; offer repair where established and retry where temporary. Missing access does not
establish that a private repository does not exist. Provider checks happen before the declaration
transaction commits and must not let a concurrent authority change invalidate what was checked.

Resolve an Integration-backed GitHub SSH/scp address to HTTPS so checkout uses the selected App's
credentials under ADR-0028. Unbound SSH sources keep their transport; accepting their syntax does
not promise that kestrel provisions SSH credentials. Shorthand alone does not choose an Integration
or grant authority: selection remains explicit, and a Workspace fixes it when it opens.

A successful GitHub check establishes only access and branch availability through the App at that
time. The supervisor still establishes actual checkout success, including network, filesystem,
transport and branch behavior in the Instance. Unbound declarations need no mandatory provider
probe. Saving their syntax must not mark them as reachable or checked out, and a later failure
uses checkout evidence rather than claiming an unsupported cause.

## Evidence and ownership

The [disposable Git probe](../investigations/repository-declaration/README.md) exercised the
supervisor's actual clone argument and destination rules. In an empty directory, `not-a-url` failed
as an absent local source; explicit local paths and a file URL cloned successfully. GitHub shorthand
was treated as a local path by Git, motivating expansion in kestrel. This was not an end-to-end
Kestrel Session, and no Session failure was claimed from the original friction log.

[Specify the GitHub connection and repository 0.4 build tickets](https://github.com/openkestrel/kestrel/issues/534)
owns the shared repository producer, provider-check and Client slices in the existing first-run
spec. Reuse selected authority, lifecycle, App credentials and typed refusals from their existing
owners. Browser setup/settings and CLI start consume the same resolved contract; no second
repository contract or validation ticket is created here.

## Considered alternatives

- URL-only validation rejects supported local and scp-style Git sources. GitHub shorthand is useful
  but needs explicit expansion before Git receives it.
- Requiring every declaration to reach its source makes offline configuration depend on the control
  plane's network and filesystem, which cannot prove Instance checkout. Selected GitHub authority
  provides a narrower, meaningful check worth requiring before save.
- Passing an Integration-backed SSH address through unchanged asks checkout for SSH authority that
  the selected App does not provide.
