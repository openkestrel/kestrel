# `@kestrel/client`

The browser Client: a static TanStack Start SPA with TanStack Query, served by the control plane's
operator listener ([ADR-0036](../../docs/adr/0036-the-browser-client-shares-the-loopback-operator-origin.md)).
It reaches the control plane only through the operator interface, on its own origin.

```sh
bun install
bun run dev        # Vite on :3000, proxying /operator to a control plane on 127.0.0.1:7718
bun run build      # dist/client, which KESTREL_CLIENT_DIR names
bun run typecheck
bun run test       # the transport
bun run e2e        # builds, then drives the built Client against `kestrel-control-plane serve`
bun run check      # biome
```

- **Types** are generated from `openapi/operator.json` by `@hey-api/openapi-ts` into
  `src/operator/generated` on every `generate`, `build` and `typecheck`; they are not committed.
  Only types are generated: `src/operator/transport.ts` owns every request, refusal and stream.
- **Routes** mirror the operator paths without `/operator`: `/organizations/$organization`,
  `…/workspaces/$workspace` and `…/new`. Each renders the three-pane `Workbench`, which becomes
  tabs under 900px.
- **Components** are shadcn's Lyra style on Base UI (`src/components/ui`) and AI Elements
  (`src/components/ai-elements`), added with their CLIs and then owned here.
