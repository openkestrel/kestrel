# Using Kestrel

Kestrel runs a coding agent against a repository in an isolated container. The work belongs to a **workspace**, which keeps its branch and transcript across sessions. A **session** is one agent conversation in that workspace. You can start one yourself, then add a trigger when you want repository events to start work for you.

This guide gets you from an empty machine to a working session. The linked guides cover the commands you will need as the work continues.

## Install and start

You need Docker with Compose v2 on an amd64 or arm64 machine.

A release attaches a `compose.yaml` that pulls that release's images, pinned by digest, and needs no checkout: download it into an empty directory and run `docker compose up -d` there. Kestrel has no release yet, so until it does, run main from a checkout:

```sh
git clone https://github.com/openkestrel/kestrel
cd kestrel
docker compose up -d
```

This pulls the images main last published to GHCR. `docker compose pull` followed by `docker compose up -d` moves to newer ones. Main does not publish them yet; until it does, [build from source](#build-from-source).

Compose starts the control plane, a filtered Docker socket proxy and the browser Client, and pulls the environment image from which Kestrel creates a container when a workspace needs one. The database lives on a named volume, so `docker compose down` preserves workspaces and their transcripts. `docker compose down --volumes` deletes that volume and its data.

The browser Client is at <http://localhost:7719>, over plain HTTP on the host's loopback, so there is no certificate to trust. `KESTREL_CLIENT_PORT=8000 docker compose up -d` publishes it on another port and tells the control plane where it moved. Each tab holds one connection for its live views, and a browser gives an origin six, so about five Kestrel tabs stay live at once; more stall until one closes.

The Client starts whether or not the control plane does. When the control plane is not answering, the Client says so and checks again on its own. Find out why with:

```sh
docker compose ps
docker compose logs kestrel
```

### Run the CLI

The control-plane image carries the CLI, so it works with nothing installed on the host:

```sh
docker compose exec kestrel kestrel status
```

To run `kestrel` on the host instead, install it with Rust through [rustup](https://rustup.rs):

```sh
cargo install --locked --path crates/kestrel-client
kestrel status
```

The host CLI connects to the operator API at `127.0.0.1:7718` by default. Opening that address in a browser shows only a note pointing at the Client. The API does not authenticate callers; Compose binds it to loopback. Use a tunnel if the control plane runs on another machine, and set `--control-plane` or `KESTREL_CONTROL_PLANE` to its URL. `docker compose logs -f kestrel` shows control plane and session diagnostics.

### Build from source

To run your own changes, build all three product images from the checkout:

```sh
docker compose --env-file compose.source.env up -d
```

`compose.source.env` names the images locally, so a `docker compose pull` never replaces what you built, and the control plane creates workspace containers from the environment image it just built. It rebuilds on every `up`; Docker's cache keeps an unchanged image quick.

## Create the GitHub App

After creating an organization (for example with `kestrel start`), run:

```sh
kestrel integration create-github-app origin --repository OWNER/REPOSITORY
```

For a repository owned by a GitHub organization, add `--app-organization OWNER` so the private App belongs to that organization. You need permission to create and install Apps there.

Open the printed URL in your browser, click **Create GitHub App**, and confirm its name on GitHub. Then follow the installation link and select the requested repository. GitHub returns you to Kestrel, which verifies the installation and registers the Integration. `kestrel integration list` shows it when setup is complete. You never download or paste the private key; Kestrel seals it and the generated webhook secret beside its database.

Complete setup within one hour. The callback uses the browser, so localhost works without exposing the operator API. If you reach Kestrel through a tunnel or the browser Client, pass its loopback origin, for example `--callback-base http://localhost:7719`. Keep that address reachable until setup finishes. If GitHub does not return after installation, use the **finish setup after installing** link in the setup tab.

The App subscribes to issues, issue comments, and pull requests, and Kestrel polls the App's delivery log every minute. Polling learns everything a webhook does, about a minute later and back as far as GitHub keeps deliveries (three days); a poll that has been away longer reports what it lost on `kestrel integration list`. If GitHub can reach your webhook listener, add `--webhook-base https://hooks.example.com` and Kestrel fills in the webhook path: deliveries then arrive at once, and the poll recognises them. Without one, the App's hook points at an address that never resolves, because GitHub refuses a localhost URL. Expose the webhook listener (7717), keeping the operator listener private. The App requests contents, issues, and pull requests read/write, and metadata read. Each Kestrel installation creates its own App.

A refused or interrupted code exchange cannot be replayed. Start a new flow if it fails; delete any unused App in GitHub's developer settings. A pending successful exchange survives a control-plane restart; revisit the setup tab's finish link within the hour.

## Start work in a repository

Run this from a clone of the repository you want the agent to work on:

```sh
ANTHROPIC_API_KEY=… kestrel start --credential ANTHROPIC_API_KEY \
  --brief "Make the README explain what this project does"
```

`start` uses the clone's origin for the repository and project name, chooses the branch from the project or clone, and uses the sole existing agent or the default OpenCode harness. It prints the values it selected and their sources before applying them. At a terminal it asks once for confirmation; use `--yes` for an unattended run. If it cannot determine a required value, it names the flag to supply. You can set `--organization`, `--project`, `--repository`, `--branch`, `--agent`, `--harness`, and `--model` explicitly.

`--credential` names an environment variable in your shell. Kestrel stores its value for the organization and supplies it to the harness when a session starts. The value does not go into the command line. The `start` command creates any missing declarations, opens a workspace, and queues its first session together. It does not silently replace an existing project or agent with different settings.

The command prints the organization, project, agent, workspace, and session it created. For scripts, `--json workspace,session` produces structured output. Use `kestrel status` to see what the control plane has and what to do next.

## Follow the session

```sh
kestrel workspace show latest
kestrel session list --workspace latest
kestrel workspace transcript latest --follow
```

`latest` means the workspace most recently opened in the selected organization. The transcript includes the brief, agent activity, and session outcome. `--follow` waits for new entries and reconnects after a control plane restart. Without it, the command prints the entries currently available and a cursor you can pass back with `--cursor` to read later entries.

A workspace keeps its checkout on its own branch, normally `kestrel/<workspace id>`. Later sessions use the same instance and checkout while they exist. If the instance has been removed, Kestrel provisions another from the remote branch; unpublished local work cannot be recovered that way.

To give the agent another turn, post a message:

```sh
kestrel workspace post latest --as-participant operator "Please add a test for the failure case"
```

A waiting session receives it in the same conversation. If no session is open, Kestrel starts a new one and supplies the workspace transcript as context. You can also start a fresh conversation with a specific agent:

```sh
kestrel session enqueue --workspace latest --agent reviewer
```

A session does not end just because the agent answered or opened a pull request. Use `kestrel session stop <session>` when you are finished with it. When the workspace is done, use `kestrel workspace seal latest`. Sealing preserves the transcript and prevents new sessions. Kestrel holds the instance if it may contain unpublished work; [Manual workspaces](docs/usage/manual-workspaces.md#sealing-a-workspace) explains how to inspect and resolve that state.

## Choose the next workflow

- [Manual workspaces](docs/usage/manual-workspaces.md) explains organizations, projects, agents, workspace and session commands, transcripts, and recovery after a restart.
- [Subscription profiles](docs/usage/subscription-profiles.md) explains how to use OpenCode, Codex, or Claude subscriptions and verify that credentials survive a restart.
- [GitHub automation](docs/usage/github-automation.md) walks through registering a repository, applying a reviewed trigger, dispatching an issue, and sending the answer back to GitHub.
- [Webhooks and schedules](docs/usage/other-triggers.md) covers other event sources and recurring work.

## Current limits

Kestrel can report a GitHub triggered session back to its issue. Generic webhooks and schedules have no return channel yet. A session that starts and fails stays failed; Kestrel retries dispatch, not the agent's work. Live collaboration in a workspace is still planned. See [the roadmap](ROADMAP.md) for the implementation sequence.
