# `@kestrel/client`

The browser Client: a static TanStack Start SPA with TanStack Query, served by a web server on the
operator interface's origin ([ADR-0043](../../docs/adr/0043-a-web-server-serves-the-browser-client.md)).
It reaches the control plane only through the operator interface, on its own origin.

```sh
bun install
bun run dev        # Vite on :3000, proxying /operator to a control plane on 127.0.0.1:7718
bun run build      # dist/client, which images/kestrel-client serves
bun run typecheck
bun run test       # the transport
bun run e2e        # builds, then drives it through images/kestrel-client/Caddyfile and `kestrel-control-plane serve`
bun run check      # oxlint and oxfmt
```

- **Types** are generated from `openapi/operator.json` by `@hey-api/openapi-ts` into
  `src/operator/generated` on every `generate`, `build` and `typecheck`; they are not committed.
  Only types are generated: `src/operator/transport.ts` owns every request, refusal and stream.
- **Routes** mirror the operator paths without `/operator`: `/organizations/$organization`,
  `…/workspaces/$workspace` and `…/new`. Each renders the three-pane `Workbench`, which becomes
  tabs below the `lg` breakpoint.
- **Components** are shadcn's Lyra style on Base UI (`src/components/ui`) and AI Elements
  (`src/components/ai-elements`), added with their CLIs and then owned here.
