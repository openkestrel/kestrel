# Automate work from GitHub

A GitHub integration records repository events and can post results back to an issue. A trigger decides which events start work, which project and agent to use, and what brief to give the agent. Set up the [project and agent](manual-workspaces.md) first.

## Register the repository

```sh
kestrel integration register github origin \
  --repository openkestrel/kestrel \
  --token ghp_your_token \
  --bot-login kestrel[bot]
```

Kestrel polls for events every minute. The first poll reads one page, establishing a starting point rather than replaying the repository's history. The `--bot-login` records the login the Integration speaks as on GitHub, so that kestrel never takes what its own identity said as input or as a command — whatever any trigger's filter admits. If GitHub can reach the control plane, add `--webhook-secret` and configure the repository webhook at the path shown by `kestrel integration list` (`/webhooks/<integration id>`, content type `application/json`). With a webhook secret, Kestrel uses deliveries instead of polling and verifies `X-Hub-Signature-256`.

`kestrel event list` shows recorded events, and `kestrel event show <record>` includes an event's payload and trigger firings. Register the integration before applying a trigger so you can inspect the event shape and test the rule against a real example.

## Declare a trigger

Keep trigger declarations in version control and review them with the repository's code. For example, this rule accepts an `@kestrel` comment from one maintainer and keeps all work on a given issue in one workspace:

```yaml
triggers:
  delegated:
    filter:
      all:
        - exact: {source: "https://github.com/openkestrel/kestrel"}
        - exact: {type: com.github.issue_comment.created}
        - any:
            - all:
                - exact: {data.user.login: jtmthf}
                - prefix: {data.body: "@kestrel"}
            - all:
                - exact: {data.comment.user.login: jtmthf}
                - prefix: {data.comment.body: "@kestrel"}
    brief: |
      {% if instruction %}{{ instruction }}{% else %}Implement{% endif %} {{ event.source }}/issues/{{ event.subject | replace("#", "") }}

      Read the issue and its comments before starting.
    branch: kestrel/issue-{{ event.subject | replace("#", "") }}
    correlation: "{{ event.source }}{{ event.subject }}"
    on_miss: open
    project: kestrel
    agent: builder
    allows: [codex, claude]
```

The two author paths account for the shapes of polled and webhook comment events. The filter chooses *who* can delegate; a `ready-for-agent` label or an ordinary comment does not start this rule. Replace the repository, login, project, agents, and brief with your own reviewed values.

Save the file as `.kestrel/triggers.yaml`, then inspect and apply it:

```sh
kestrel trigger apply -f .kestrel/triggers.yaml --dry-run
kestrel trigger apply -f .kestrel/triggers.yaml
```

`apply` prints a diff and treats the file as the complete applied trigger set for the organization. Removing a trigger from the file removes that applied trigger. Keep one declaration file per organization; a second partial file can remove triggers declared by the first. An apply with a missing project or agent changes nothing. A trigger declared directly with `kestrel trigger declare` stays separate unless the file takes over its name.

A filter can compare CloudEvents fields and paths into `data` with `exact`, `prefix`, or `suffix`, combining tests with `all`, `any`, and `not`. Comparisons are case sensitive; a missing data path does not match. The `brief`, `branch`, and `correlation` use [MiniJinja](https://docs.rs/minijinja) templates over `event`. Rendering fails if a referenced field is absent. Guard optional fields with `is defined`.

The trigger's declared project and agent determine its authority. Event data supplies values for the brief and branch; it cannot name an arbitrary agent or credential. `allows` is a reviewed list from which an `agent:codex` issue label, or an `agent=codex` command immediately after `@kestrel`, may choose. Conflicting labels or a name outside that list fail the firing. See [ADR-0013](../adr/0013-an-event-supplies-data-never-authority.md).

## Try a trigger without a file

For an experiment, declare a trigger directly. This example covers polled comments by one author; adapt the filter to your event shape before enabling it:

```sh
kestrel trigger declare ready \
  --filter '{"all":[{"exact":{"source":"https://github.com/openkestrel/kestrel"}},{"exact":{"type":"com.github.issue_comment.created"}},{"exact":{"data.user.login":"jtmthf"}}]}' \
  --brief @.kestrel/briefs/ready.md \
  --project kestrel \
  --agent builder
```

`--filter` and `--brief` accept inline text, `@path` for a file, or `-` for standard input. You can add `--branch`, `--correlation`, `--on-miss`, and `--on-open-workspace` as you would in an applied declaration. An applied file leaves this trigger alone until it declares the same name. `kestrel trigger show ready` reports whether it came from a file.

## Test before handing over work

Test against an event already recorded, or against an issue fetched through the integration:

```sh
kestrel trigger test delegated --event <event-record>
kestrel trigger test delegated --integration origin --issue 44 \
  --instruction 'Implement the fix' --agent codex
```

A test starts no work. It reports whether the filter matches and shows the selected agent, branch, correlation, and rendered brief. Add `-f .kestrel/triggers.yaml` to test edits before applying them. For an issue dispatch, `matches` is always true: dispatch deliberately fires the named trigger regardless of its event filter.

A trigger fires once per recorded event and only for events recorded after its declaration or change. Changing a filter does not replay old events. If several triggers match one event, each fires. `kestrel trigger disable delegated` pauses one rule; `kestrel trigger enable delegated` restores it. Each trigger has a limit of ten firings per hour; exceeding it records the firing and disables the trigger until an operator enables it again.

Review filters that allow outside contributors to delegate work. Until policy enforcement is available beneath a session, such a trigger gives their input to an agent running with the organization's repository credentials. Kestrel warns when a GitHub filter does not constrain the author to an organization member, collaborator, or named login.

## Give the agent GitHub tools

The base `kestrel-env` image contains OpenCode and git, but no `gh` command. To let the agent read issues or open a pull request, build the supplied [`kestrel-dev` image](../../images/kestrel-dev/README.md) and point the control plane at it:

```sh
docker compose build kestrel-env
docker build --file images/kestrel-dev/Dockerfile --tag kestrel-dev .
```

In a gitignored `compose.override.yaml`:

```yaml
services:
  kestrel:
    environment:
      KESTREL_IMAGE: kestrel-dev
```

```sh
docker compose up -d
gh auth token | kestrel credential set GH_TOKEN
```

`KESTREL_IMAGE` selects the image used for workspace instances. `GH_TOKEN` is the variable `gh` reads inside a session; Kestrel stores the token from standard input. The token's repository access can also permit a merge. If you require human approval before merging, enforce it with a distinct reviewer identity or a repository rule; Kestrel does not narrow the token's permissions.

## Dispatch and continue an issue

A matching `@kestrel` comment starts the trigger's work. Text following the mention becomes `instruction` in the brief; the mention must start the comment. For example:

```text
@kestrel agent=codex Add a regression test and open a pull request
```

You can also dispatch an issue explicitly, regardless of whether it has a matching comment:

```sh
kestrel trigger dispatch delegated --integration origin --issue 44 \
  --instruction 'Implement the fix' --agent codex
```

A dispatch records its own event and fires only the trigger you named. `--instruction` accepts text, `@FILE`, or `-` for standard input. The same correlation and firing limit apply. Inspect `kestrel workspace list` and `kestrel workspace transcript <workspace>` to follow the result.

The correlation key ties later events for an issue to its open workspace. With `on_open_workspace: continue` (the default), another firing becomes a waiting session's next turn, or starts a session if needed. `on_open_workspace: new-session` starts a new conversation on the same checkout after the previous turn finishes. If the correlated workspace has sealed, Kestrel opens a new workspace that continues it. `on_miss: ignore` records a firing without opening work for a new key.

A normal follow-up comment from an author allowed by the trigger becomes a message in the open workspace. If the agent is mid-turn, messages wait durably. A comment on a sealed workspace starts no work unless it is a new command. The first brief and later messages appear in the transcript.

The agent opens a pull request with its own `gh` call. Kestrel declares the branch and can learn about the pull request from the GitHub integration's events; the control plane does not run git or compose the pull request. It posts each completed turn's answer back to the issue. If a session fails or ends without an answer, Kestrel posts the outcome. Outbound comments carry markers so delivery after a restart does not duplicate them. Register the integration with `--carries inbound` if it should observe events without writing comments.
