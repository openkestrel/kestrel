# Using Kestrel

Kestrel runs a coding agent against a repository in an isolated container. The work belongs to a **workspace**, which keeps its branch and transcript across sessions. A **session** is one agent conversation in that workspace. You can start one yourself, then add a trigger when you want repository events to start work for you.

This guide gets you from a source checkout to a working session. The linked guides cover the commands you will need as the work continues.

## Install and start

You need Docker with Compose v2, an amd64 or arm64 machine, and Rust through [rustup](https://rustup.rs). The first start builds the images from source and can take several minutes.

```sh
git clone https://github.com/openkestrel/kestrel
cd kestrel
docker compose up -d
cargo install --locked --path crates/kestrel-client
kestrel status
```

Compose starts the control plane and a filtered Docker socket proxy. It also builds the environment image from which Kestrel creates a container when a workspace needs one. The database lives on a named volume, so `docker compose down` preserves workspaces and their transcripts. `docker compose down --volumes` deletes that volume and its data.

The CLI connects to the operator API at `127.0.0.1:7718` by default. That API does not authenticate callers; Compose binds it to loopback. Use a tunnel if the control plane runs on another machine, and set `--control-plane` or `KESTREL_CONTROL_PLANE` to its URL. `docker compose logs -f kestrel` shows control plane and session diagnostics.

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
kestrel workspace post latest "Please add a test for the failure case"
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
