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
| `ship-image.sh` | — | build the image locally and ship it to vya2 (see runbook step 1) |

The freeflight **nginx vhost** is *not* kept here — it lives in the
separate `vya-ws/nginx` repo (`vya-ws/nginx/conf.d/freeflight.conf`),
which is the source of truth for vya2's shared nginx and is deployed with
its own `deploy.sh`. See "nginx config change" in the runbook below.

The nginx `docker-compose.yml` (also in `vya-ws/nginx`) carries the
static-root volume line: `- /var/www/freeflight:/srv/freeflight:ro`.

## Redeploy runbook

All commands run as `root@vya2.flyvoyager.net` unless noted.

**1. ff-api (code change).** Build the image **locally** and ship the
image itself — vya2 no longer builds from source. One command from a
local checkout:

```sh
sh deploy/ship-image.sh
```

It runs `docker build` locally (fast — the committed `.dockerignore`
keeps the ~19 GB `data/` cycle out of the build context), then
`docker save ff-api:latest | gzip | ssh root@vya2 'gunzip | docker load'`
(~33 MB over the wire; there's no registry, `pull_policy: never`), and
finally `docker compose up -d` on the server to recreate the container
with the new image. Both ends are linux/amd64.

The old flow (scp a `git archive` source tarball → `docker build` on the
server, ~20 min cold) still works if you ever can't build locally, but
the local-build-and-ship path is the default now.

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

**3a. Cycle with non-US (France / SIA AIXM) data.** The pipeline folds in
French airports/navaids/waypoints/runways/airways/airspace from the SIA
AIXM 4.5 export when `FF_AIXM_FR_PATH` points at it (DESIGN.md §3.1). This
is opt-in — a normal cycle build (step 3) omits it. To publish an
**up-to-date** France-inclusive cycle:

1. **Match the AIRAC cycle.** The FAA and SIA both follow the global ICAO
   AIRAC calendar (28-day, synchronized effective dates), so the France
   data must be from the **same cycle** the FAA pipeline pulls. The
   pipeline auto-discovers the current FAA CIFP cycle; the SIA export you
   feed it has to have the matching effective date, or the bundle will
   label itself with the FAA date while carrying stale France data.

2. **Download the matching SIA export.** From
   <https://www.sia.aviation-civile.gouv.fr> → *Produits numériques en
   libre disposition* → *Bases de données SIA*, add the current
   *Données aéronautiques XML AIRAC* product to the cart (free, 0,00 €),
   check out, and download `export_xml_bd_SIA<date>.zip`. Confirm its
   validity dates cover the FAA cycle. Licence: **Licence Ouverte** —
   redistribution OK, **attribution required** (see step 5).

3. **Build the cycle** on a GDAL-capable host (vya2 has no GDAL; WSL/Debian
   does — that's where this was validated). GDAL's `gdal_translate`/
   `gdalwarp`/`gdaladdo` must be on `PATH`:

   ```sh
   export FF_AIXM_FR_PATH=/path/to/export_xml_bd_SIA<date>.zip
   export FF_ETL_DATA_DIR=/path/to/output          # NOT the repo data/ unless intended
   export RUST_LOG=info
   cargo run --release -p ff-etl --bin ff-etl      # note: --bin ff-etl (crate has several)
   ```

   Same run as step 3, plus one early log line to check:
   `added France/SIA AIXM data to bundle ... airports=… airspaces=…`.

4. **Verify AIRAC alignment.** In the log, confirm `fetched CIFP
   cycle=<date>` matches your SIA export's cycle. (Observed once: the FAA
   rolled to `2026-08-06` while the SIA file on hand was `2026-07-09` — a
   one-cycle mismatch. Re-download the matching SIA export rather than
   publish that.)

5. **Attribution prerequisite (Licence Ouverte).** Before a France-
   inclusive cycle goes live, the web client must display
   "Service de l'Information Aéronautique (SIA)" **and the export's
   effective date**. The About page already credits the SIA (`apps/web`),
   but the per-cycle date is not wired through yet — finish that first, or
   you're shipping the data without meeting the licence's attribution
   condition.

6. **Publish** exactly as step 3: copy `data/cycles/<id>/` (~19 GB) +
   `data/latest.json` to `/containers/freeflight/data/`, then
   `docker compose restart`.

Known limitation: navaid/waypoint `region` is stamped `LF` for the whole
`FR_OM` export, which actually spans several ICAO regions (metropolitan,
New Caledonia, Antilles, …) — cosmetic; per-feature region is a TODO.

**4. nginx config change.** The vhost lives in the separate `vya-ws/nginx`
repo, **not** here — edit `vya-ws/nginx/conf.d/freeflight.conf` and deploy
it from that repo (Git Bash has no rsync, so go through WSL):

```sh
cd ../../vya-ws/nginx && wsl bash ./deploy.sh
```

`deploy.sh` rsyncs `conf.d/` to `root@vya2:/containers/nginx/`, runs
`docker exec nginx nginx -t`, and only reloads (`nginx -s reload`) if the
config validates — so a bad config never goes live. Don't edit the file
on the server directly; that drifts from the source of truth.

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
