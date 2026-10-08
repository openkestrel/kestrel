# Repository declaration probe

Recorded on 2026-10-06 with Git 2.55.0. The examined checkout/declaration code was on the
`decision/0.4-integration-lifecycle` branch at `18c37f3`; this evidence was copied into the
repository declaration decision branch. Temporary paths below identify the disposable probe,
not paths an installed kestrel should use.

This is a disposable actual Git clone probe, not an end-to-end Kestrel Session test. No Cargo builds, repository edits, tracker writes or user deployment changes were made. A bare local repository with a seeded `main` commit was created under this directory. Each case ran in its own empty directory, using the same argument shape as `checkout::check_out`:

`git clone --branch main <repository> <directory>`

Directory was derived with Kestrel's rule: trim trailing slashes, take last slash-separated component, remove final `.git`. Git ran with `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, and isolated global/system Git configuration. External probes used loopback addresses, not real hosting services. An eight-second subprocess timeout bounded probes; none timed out.

| Input | Destination | Exit | Observed evidence |
| --- | --- | --- | --- |
| `/private/tmp/kestrel-repository-probe-ek_y8yx1/source.git` | source | 0 | Cloning into 'source'... done. |
| `file:///private/tmp/kestrel-repository-probe-ek_y8yx1/source.git` | source | 0 | Cloning into 'source'... |
| `../source.git` | source | 0 | Cloning into 'source'... done. |
| `not-a-url` | not-a-url | 128 | fatal: repository 'not-a-url' does not exist |
| `github.com/acme/widgets.git` | widgets | 128 | fatal: repository 'github.com/acme/widgets.git' does not exist |
| `acme/widgets` | widgets | 128 | fatal: repository 'acme/widgets' does not exist |
| `https://127.0.0.1:1/acme/widgets.git` | widgets | 128 | Failed to connect to 127.0.0.1 port 1; HTTPS transport attempted |
| `ssh://git@127.0.0.1:1/acme/widgets.git` | widgets | 128 | ssh: connect to host 127.0.0.1 port 1: Operation not permitted; SSH transport attempted |
| `git@127.0.0.1:acme/widgets.git` | widgets | 128 | ssh: connect to host 127.0.0.1 port 22: Operation not permitted; SSH transport attempted |
| `git://127.0.0.1:1/acme/widgets.git` | widgets | 128 | fatal: unable to connect to 127.0.0.1; errno=Operation not permitted; git transport attempted |
| empty string | empty string | 128 | fatal: repository '' does not exist |

A second probe created an existing bare local repository named `not-a-url`:

- From its parent, `git clone --branch main not-a-url not-a-url` exits 128: `fatal: destination path 'not-a-url' already exists and is not an empty directory.`
- From another directory, `git clone --branch main ../same-string-existing/not-a-url not-a-url` exits 0: `Cloning into 'not-a-url'... done.`

These show that the string can be a local path, while Kestrel's destination naming can make a same-directory clone fail. The first not-a-url failure is therefore evidence of an absent local source in that working directory, not a universal Git syntax rejection.

Code: `crates/kestrel-supervisor/src/checkout.rs:11` and `:292` define the command and directory; `:304` and `:309` wrap stderr and set the environment. `lib.rs:827` marks a checkout failure finished and sends Checkout then Finished/Failed reports. The predicted Kestrel failure text from this code is `not-a-url could not be checked out on the branch <branch>: fatal: repository 'not-a-url' does not exist`; it was not observed through a live Kestrel Session.

## Declaration and identity findings

Project declaration (`crates/kestrel/src/operator.rs`), document application (`declaration.rs`)
and first-start planning (`start.rs`) check that the repository list and branch are nonempty,
and that derived checkout directories do not collide. They do not validate each repository
string or require reachability. The CLI accepts raw repository values and also infers the local
clone's raw origin; Project storage preserves them, and a Workspace fixes its checkout when opened.

GitHub identity matching (`integration/github.rs::named_repository`) recognizes only fixed
github.com prefixes for HTTP(S), SSH with `git` as user, scp-style `git@github.com:` and `git://`,
followed by `owner/name`. It does not normalize the stored repository or choose an Integration
for custom hosts, local paths or file URLs. The new explicit authority decision in ADR-0056
therefore needs a shared resolved repository contract rather than another repository-string lookup.

[Git's clone documentation](https://git-scm.com/docs/git-clone) confirms built-in HTTP(S), SSH,
scp-style, git and local/file transports, and distinguishes helper transports from those forms.
The probe demonstrates actual successful local/file clones and failed loopback transport attempts;
it does not establish successful hosted HTTPS/SSH clones or the presence of local paths inside a
provisioned Instance.
