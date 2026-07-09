# Deployment — freeflight.flyvoyager.net

Production deployment of freeflight, hosted on **vya2.flyvoyager.net**
(Debian 13, Docker + Compose). Mirrors the existing sibling services on
that host (`butterbot`, `missiongen`, `vfaa`): locally/server-built
images, no registry (`pull_policy: never`), all joined to a shared
external Docker network so one central nginx terminates TLS and
reverse-proxies to each.

## Topology

```
                         Internet
                            │
                            ▼
                   ┌─────────────────┐
                   │   Cloudflare    │  proxied DNS + edge TLS
                   │  (orange cloud) │  freeflight.flyvoyager.net
                   └────────┬────────┘  A → 51.159.58.182
                            │ HTTPS (Origin CA wildcard *.flyvoyager.net)
                            ▼
┌──────────────────────── vya2.flyvoyager.net ─────────────────────────┐
│                                                                       │
│   ┌──────────────────────────┐                                       │
│   │  nginx  (container)       │  :80 → 301 → :443                     │
│   │  /containers/nginx/       │  TLS: certs/flyvoyager.net.pem        │
│   │  networks: web, vya2net   │                                       │
│   │                           │                                       │
│   │  vhost freeflight.conf:   │                                       │
│   │   location /              ├──► /srv/freeflight  (static files)    │
│   │     (SPA static)          │      = /var/www/freeflight  ◄─ bind   │
│   │                           │                                       │
│   │   location ~ ^/(data|     │                                       │
│   │     bundles|weather|      ├──► http://freeflight-api:8080         │
│   │     notams|cycles|health) │      (over vya2net, by container name)│
│   └──────────────────────────┘                                       │
│                            │                                          │
│                            ▼  vya2net                                 │
│   ┌──────────────────────────┐                                       │
│   │  freeflight-api           │  ff-api (axum), listens :8080         │
│   │  image ff-api:latest      │  FF_ETL_DATA_DIR=/data  PORT=8080     │
│   │  /containers/freeflight/  │                                       │
│   │  networks: vya2net        │                                       │
│   │                           ├──► /data  (ro bind mount)             │
│   └──────────────────────────┘      = /containers/freeflight/data    │
│                                        └ cycles/2026-07-09/           │
│                                            cycle.sqlite (139 MB)      │
│                                            chart-*.pmtiles (~19 GB)   │
└───────────────────────────────────────────────────────────────────────┘
```

## Request routing

The web client is built with `VITE_FF_API_BASE_URL=https://freeflight.flyvoyager.net`,
so it and its data live under **one hostname**. nginx splits by path:

| Path prefix | Handled by | Notes |
|---|---|---|
| `/` and everything else | static `/srv/freeflight` | SPA; `try_files … /index.html` |
| `/data/*` | `ff-api` | JSON query endpoints (airports, search, procedures, airways, charts catalog, airspace) |
| `/bundles/*` | `ff-api` | `cycle.sqlite` + `chart-*.pmtiles`; served with HTTP **Range** support (PMTiles) |
| `/weather/*` | `ff-api` | METAR/TAF/G-AIRMET/SIGMET/PIREP/winds-aloft proxy |
| `/notams` | `ff-api` | NOTAM proxy |
| `/cycles/latest` | `ff-api` | active cycle manifest |
| `/health` | `ff-api` | liveness |

## TLS / DNS

- The cert is a **Cloudflare Origin CA wildcard** `*.flyvoyager.net`
  (`/containers/nginx/certs/flyvoyager.net.pem`). Browsers only trust it
  behind Cloudflare, so the DNS record **must be Proxied (orange cloud)**.
  Grey-cloud / DNS-only would fail cert validation.
- DNS record: `freeflight` **A** → `51.159.58.182` (vya2), proxied.
  Optionally `AAAA` → `2001:bc8:1200:2:208:a2ff:fe0c:7926`.

## Files in this directory

| File | Installed to (vya2) | Purpose |
|---|---|---|
| `Dockerfile` | build context = workspace root | multi-stage Rust build of `ff-api` |
| `compose.yml` | `/containers/freeflight/compose.yml` | runs the `freeflight-api` container |
| `nginx-freeflight.conf` | `/containers/nginx/conf.d/freeflight.conf` | the vhost |

The nginx `docker-compose.yml` also gains one volume line:
`- /var/www/freeflight:/srv/freeflight:ro`.

## Redeploy runbook

All commands run as `root@vya2.flyvoyager.net` unless noted.

**1. ff-api (code change).** From a local checkout, ship the Rust
workspace source (no `target/`, `node_modules/`, `data/`) to
`/containers/freeflight/build/src`, then on the server:

```sh
cd /containers/freeflight/build/src
docker build -f deploy/Dockerfile -t ff-api:latest .   # ~20 min cold
cd /containers/freeflight && docker compose up -d       # recreate container
```

**2. Web client (UI change).** Locally:

```sh
cd apps/web
VITE_FF_API_BASE_URL=https://freeflight.flyvoyager.net npm run build
```

Ship `dist/` → `/var/www/freeflight` (replace contents). No restart
needed — nginx serves the files directly.

**3. Data cycle (new cycle).** Regenerating a cycle needs `ff-etl` +
GDAL CLI tools, which vya2 does **not** have — build the cycle where
GDAL is available and copy the `data/cycles/<id>/` dir (~19 GB) plus
`data/latest.json` to `/containers/freeflight/data/`. A resumable
per-file `scp` loop works when `rsync` isn't available on the client
(see `scripts`/session notes). Then:

```sh
cd /containers/freeflight && docker compose restart   # picks up new latest.json
```

**4. nginx config change.** Edit `/containers/nginx/conf.d/freeflight.conf`,
then:

```sh
docker exec nginx nginx -t        # validate
cd /containers/nginx && docker compose up -d   # or: docker exec nginx nginx -s reload
```

## Health checks

```sh
curl https://freeflight.flyvoyager.net/health              # {"status":"ok"}
curl "https://freeflight.flyvoyager.net/data/search?q=KSFO"
curl https://freeflight.flyvoyager.net/cycles/latest
curl -H 'Range: bytes=0-16' -o /dev/null -w '%{http_code}\n' \
  https://freeflight.flyvoyager.net/bundles/2026-07-09/chart-albuquerque.pmtiles   # 206
```

## Notes / limitations

- Single instance, no HA — a hobby deployment (DESIGN.md §8).
- The web client is connectivity-assuming and has no offline mode; if
  `ff-api` is unreachable it says so and stops.
- NOTAM/weather proxying uses `ff-api`'s upstream credentials
  (`FF_NOTAM_CLIENT_ID`/`FF_NOTAM_CLIENT_SECRET` env, currently unset);
  those overlays degrade independently and don't affect the core app.
