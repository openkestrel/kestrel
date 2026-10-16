# Reach GitHub's App hook

Every inbound GitHub Integration continuously polls the App's Delivery log. A reachable hook
reduces latency; it never replaces polling. With no public base, a new App keeps the accepted
`https://unreachable.invalid/webhooks/…` URL. A loopback hook is never a default.

Set `KESTREL_WEBHOOK_BASE=https://hooks.example.com` (or control-plane `--webhook-base`) before
starting kestrel to apply that public HTTPS origin to new Apps. An explicit base supplied during
App creation overrides it. Restarting with a different base does not alter existing Apps.

For an existing Integration:

```sh
kestrel integration github webhook set-url github --base https://hooks.example.com
kestrel integration github webhook status github
```

Omit `--base` to use the control plane's deployment setting. `--revision N` refuses a decision
made against an older Integration read. Both commands support the ordinary `--json` and embedded
filter options. The URL update changes only GitHub's hook URL, preserving its secret, event
subscriptions and polling cursor. It refuses retired authority, unfinished secret rotation and an
App used by another non-retired Integration, including disabled ones in other Organizations.

The shared operator endpoint is `GET`/`PUT
/operator/organizations/{organization}/integrations/{integration}/webhook`. Settings Clients can
consume its generated contract. PUT returns the Integration and its new revision; this only means
GitHub acknowledged configuration. GET reads the configured URL and the latest page of Delivery
results from GitHub, scoped to the Integration's repository and installation. A `2xx` Delivery
result is evidence of an HTTP acknowledgement at that time; an empty log or a configured URL
proves no reachability. Read refusals are not empty logs. A timed-out update may have landed:
inspect the hook before retrying. The endpoint never returns the signing secret or Delivery payload.

## Public server

Keep the operator API on loopback `127.0.0.1:7718` and the browser Client on loopback
`127.0.0.1:7719`. Keep kestrel's link/webhook listener on `127.0.0.1:7717`. Point your own DNS at
a server you operate and terminate HTTPS with a reverse proxy, for example Caddy:

```caddyfile
hooks.example.com {
    handle /webhooks/* {
        reverse_proxy 127.0.0.1:7717
    }
    handle {
        respond 404
    }
}
```

Only the webhook paths are public. The proxy does not publish `/link`, the operator API or the
Client. Allow inbound HTTPS to the proxy; do not open 7717, 7718 or 7719 publicly. For Compose,
publish the control plane's 7717 on host loopback in a local override and supply
`KESTREL_WEBHOOK_BASE` in that service's environment; keep the existing operator/Client loopback
bindings. Access a remote operator or Client separately through an SSH local forward.

## Tunnel to a local install

On a public server you operate, use the same Caddy configuration with the reverse-proxy target
changed to `127.0.0.1:17717`. From the machine running kestrel, explicitly start an SSH reverse
forward:

```sh
ssh -N -o ExitOnForwardFailure=yes -R 127.0.0.1:17717:127.0.0.1:7717 you@your-server
```

The SSH listener stays on the server's loopback; only the proxy's `/webhooks/*` route is exposed.
Configure the App base as `https://hooks.example.com`. Stopping the tunnel loses webhook speed
while polling continues. This uses your own server, no third-party relay; kestrel never starts or
manages a tunnel.
