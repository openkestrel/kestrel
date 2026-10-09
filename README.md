<p align="center">
  <img src="docs/assets/kestrel-logo.png" alt="Kestrel logo" width="300">
</p>

<h1 align="center">Kestrel</h1>

<p align="center"><strong>Run coding agents on your own infrastructure.</strong></p>

Kestrel starts agent work from the CLI, GitHub events, webhooks, or schedules. Each session runs in a container. Its workspace keeps the branch and transcript, so you can inspect the work and continue it in a later session.

**Project status:** Kestrel is an unreleased prototype. The local Docker Compose deployment supports manual and event-driven sessions. GitHub-triggered sessions can report answers to the originating issue. See the [roadmap](ROADMAP.md) for planned capabilities.

## Get started

You need Docker with Compose v2 and an amd64 or arm64 machine.

```sh
git clone https://github.com/openkestrel/kestrel
cd kestrel
docker compose up -d
docker compose exec kestrel kestrel status
```

Compose pulls the images main last published and opens the browser Client at <http://localhost:7719>. A release will attach its own `compose.yaml` instead, and contributors build the images from source; until main publishes its images, so does everyone (`docker compose --env-file compose.source.env up -d`). The [usage guide](USAGE.md) covers all three, then your first session.

## Documentation

- [Usage guide](USAGE.md) — first session and links to manual workspaces, subscriptions, and automation.
- [Roadmap](ROADMAP.md) — current milestones and planned work.
- [Domain model](GLOSSARY.md) — the terms used throughout the project.

Kestrel is licensed under [Apache 2.0](LICENSE).
