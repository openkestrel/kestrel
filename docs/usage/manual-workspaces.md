# Manual workspaces

The [quick start](../../USAGE.md) creates the declarations and queues a session in one step. Use the commands here when you want to manage those pieces separately or return to work already under way.

## Declare an organization, project, and agent

An organization contains the projects, agents, credentials, and workspaces you manage together. A project names the repositories and base branch for new workspaces. An agent selects the harness and model used by a session.

```sh
kestrel organization declare acme
kestrel project declare kestrel \
  --repository https://github.com/openkestrel/kestrel \
  --branch main
kestrel agent declare builder
```

With one organization, the CLI selects it automatically. With more than one, set `--organization`, `KESTREL_ORGANIZATION`, or a committed `.kestrel/organization` file. `organization list`, `project list`, and `agent list` show the current declarations. Repeating a declaration leaves an unchanged record alone; project and agent declarations with the same name update that record in place.

The default harness is OpenCode. `--harness codex` and `--harness claude` are also available when the environment image contains their ACP adapters; the supplied [`kestrel-dev` image](../../images/kestrel-dev/README.md) does. With no `--model`, the harness chooses its default. To change the model for future sessions:

```sh
kestrel agent model builder --model anthropic/claude-opus-4-5
```

An Agent, a Trigger, `workspace open` and `session enqueue` each declare a model, a mode and a thought level, each a harness value id. Each category resolves on its own: the Session's own value over its Trigger's over its Agent's, and a category none of them names stays at the harness's default. `workspace open` and `session enqueue` take `--model`, `--mode` and `--thought-level`; Agent and Trigger declaration files take `model`, `mode` and `thought_level`. A session keeps the harness and options selected when it was queued. If its harness cannot provide a declared value — no way to set that category, or one that does not offer the value — the session fails at setup naming the category and the value. The control plane's `KESTREL_HARNESS_COMMANDS` setting or repeated `--harness-command NAME=COMMAND` arguments can replace the default command table.

An organization can limit live instances with `--max-live-instances N`. When that limit is reached, Kestrel archives clean idle instances oldest first. Work waits if every idle instance may hold unpublished changes. Follow-up work in an existing workspace continues to use its instance.

## Open a workspace and queue a session

```sh
kestrel workspace open --project kestrel --agent builder
kestrel workspace show latest
kestrel workspace post latest --as-participant operator "Explain the failing test and propose a fix"
```

Posting the first task queues a session. You can also run `kestrel session enqueue --workspace latest` explicitly; without a brief or message, the harness receives Kestrel's general context prompt rather than a specific task.

A workspace records its project's repositories at the time it opens. It normally uses a new `kestrel/<workspace id>` branch cut from the project's base branch. Supply `--branch` to work on an existing branch. The supervisor clones the repositories and checks out the branch before starting the harness. A failed checkout ends the session with the repository and branch in the error.

The workspace identifier, any unique prefix of it, its generated name, or `latest` can be used in subsequent commands. Only one session can be open in a workspace. The default work limit is two sessions actively taking turns across all workspaces; sessions waiting for another turn retain their conversation and instance without occupying an active slot. Set `KESTREL_MAX_ACTIVE_SESSIONS` or `--max-active-sessions` to change the limit.

```sh
kestrel session list --workspace latest
kestrel workspace transcript latest --follow
kestrel workspace transcript latest --follow --as-participant Ada
```

A transcript without `--follow` ends with a cursor on stderr. Pass it to `--cursor` to get only later entries. With `--follow`, the CLI reconnects and resumes after the last printed entry if the control plane drops. `--as-participant NAME` joins the workspace's presence under that name while following, and renews its one-minute lease every 20 seconds; presence is never printed. Harness stderr goes to `docker compose logs -f kestrel`, labeled by session, and stays out of the transcript.

The CLI renders readable columns at a terminal and tab-delimited records when piped. `--json id,state` selects fields for scripts; use exit codes rather than parsing diagnostic text. `kestrel exit-codes` lists them.

## Continue the work

```sh
kestrel workspace post latest --as-participant operator "Please add the missing test"
```

If a session is waiting, the message becomes its next turn. If it is working, the message waits in the transcript until the turn finishes. If no session is open, posting queues another session and supplies the earlier transcript as context. To use a different agent in a new conversation on the same checkout, run:

```sh
kestrel session enqueue --workspace latest --agent reviewer
```

The session's agent, harness, and model appear in `kestrel session show`. Stop a session with `kestrel session stop <session>`: one waiting between turns succeeds; one stopped mid-turn or before starting fails.

## Restarts and recovery

A workspace and its transcript survive a control plane restart. An in-flight session ends with a recorded failure, while its instance and checkout remain available for another session. If an instance is missing, the next attempt reports that local unpublished work may have been lost; a later session can provision a fresh instance from the remote branch.

```sh
docker compose down
docker compose up -d
kestrel workspace show latest
kestrel workspace transcript latest
```

The database uses a named volume. `docker compose down --volumes` removes that volume, including declarations, workspaces, and transcripts.

## Sealing a workspace

Seal a workspace when its work is complete:

```sh
kestrel workspace seal latest
```

A sealed workspace stays readable but accepts no new sessions. Kestrel archives its instance only when the last session reported a checkout that the remote can restore. Untracked or uncommitted files, stashes, and unpushed commits hold the instance. Ignored build output does not. An interrupted session that never reported checkout state also holds the instance.

```sh
kestrel instance list
kestrel workspace show latest
```

Both commands show why an instance is held. To keep the work, continue the workspace and ask the agent to commit and push it. To discard it, release the instance, then seal:

```sh
kestrel instance release latest
kestrel workspace seal latest
```

Releasing destroys the instance and any work held only there. A workspace with a session in a turn or still queued cannot seal until the turn finishes. A waiting session ends when its workspace seals.

Kestrel also seals open workspaces after 24 hours without activity or a turn in flight. A held instance keeps its workspace open. To continue from a sealed workspace, open a new one and record the relationship:

```sh
kestrel workspace open --project kestrel --agent builder --continues <sealed-workspace>
```
