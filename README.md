<p align="center">
  <img src="docs/assets/kestrel-logo.png" alt="Kestrel logo" width="300">
</p>

<h1 align="center">Kestrel</h1>

<p align="center"><strong>Run coding agents on your own infrastructure.</strong></p>

Kestrel starts agent work from the CLI, GitHub events, webhooks, or schedules. Each session runs in a container. Its workspace keeps the branch and transcript, so you can inspect the work and continue it in a later session.

**Project status:** Kestrel is an unreleased prototype. The local Docker Compose deployment supports manual and event-driven sessions. GitHub-triggered sessions can report answers to the originating issue. See the [roadmap](ROADMAP.md) for planned capabilities.

## Get started

You need Docker with Compose v2, an amd64 or arm64 machine, and Rust installed through [rustup](https://rustup.rs).

```sh
git clone https://github.com/openkestrel/kestrel
cd kestrel
docker compose up -d
cargo install --locked --path crates/kestrel-client
kestrel status
```

The first start builds the container images from source. Follow the [usage guide](USAGE.md) to run your first session and inspect its work.

## Documentation

- [Usage guide](USAGE.md) — first session and links to manual workspaces, subscriptions, and automation.
- [Roadmap](ROADMAP.md) — current milestones and planned work.
- [Domain model](GLOSSARY.md) — the terms used throughout the project.

Kestrel is licensed under [Apache 2.0](LICENSE).
