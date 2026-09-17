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
| `Dockerfile.chart-hash-backfill` | build context = workspace root | one-off `backfill_chart_hashes` (see "Chart hashes" below) |
| `compose.yml` | `/containers/freeflight/compose.yml` | runs the `freeflight-api` container |
| `ship-image.sh` | — | build the image locally and ship it to vya2 (see runbook step 1) |
| `backup-db.sh` | `/containers/freeflight/backup-db.sh` | daily `pg_dump` of the account DB → local + Dedibackup (see "Account database" below) |
| `freeflight-backup.service` | `/etc/systemd/system/` | oneshot unit that runs the above, with a Discord notification on failure |
| `freeflight-backup.timer` | `/etc/systemd/system/` | runs it daily at 03:30 |

The freeflight **nginx vhost** is *not* kept here — it lives in the
separate `vya-ws/nginx` repo (`vya-ws/nginx/conf.d/freeflight.conf`),
which is the source of truth for vya2's shared nginx and is deployed with
its own `deploy.sh`. See "nginx config change" in the runbook below.

The nginx `docker-compose.yml` (also in `vya-ws/nginx`) carries the
static-root volume line: `- /var/www/freeflight:/srv/freeflight:ro`.

## Abuse resistance: rate limiting and CORS

`ff-api` is a public, unauthenticated proxy, so DESIGN.md §11 requires
per-IP rate limiting on the routes that spend an upstream budget, and CORS
restricted to our own origins. Both are on by default; the deployment only
has to tell them where the real client address comes from.

| Variable | Set it to | Why |
|---|---|---|
| `FF_TRUSTED_CLIENT_IP_HEADER` | `cf-connecting-ip` | **Important.** See below. |
| `FF_WEB_ORIGINS` | `https://freeflight.flyvoyager.net` | CORS allowlist, shared with the OAuth redirect guard. Localhost is always allowed. |
| `FF_RATE_LIMIT_RPS` | *(unset → 2)* | Sustained requests/sec per client on `/weather/*` and `/notams`. `0` disables limiting. |
| `FF_RATE_LIMIT_BURST` | *(unset → 30)* | Back-to-back allowance from idle; covers one map pan's worth of weather calls. |

**The one that matters.** `ff-api` sees nginx's `vya2net` container
address as the socket peer, identical for every request on earth. Left
unset, the limiter keys on that, so the whole internet shares one bucket
and real users start getting 429s at a combined ~2 req/s — which looks
like the service is broken, not misconfigured. The service logs a warning
on the first such request, but set the variable and don't rely on
noticing it:

```
FF_TRUSTED_CLIENT_IP_HEADER=cf-connecting-ip
```

in `/containers/freeflight/.env`, then `docker compose up -d`.

That header is only trusted when configured, because trusting it blindly
would let anyone bypass the limit with a fresh value per request — so
**check nginx actually sets it before turning it on.** Cloudflare sends
`CF-Connecting-IP` to the origin; the freeflight vhost must pass it
through (`proxy_set_header CF-Connecting-IP $http_cf_connecting_ip;`, or
rely on the header arriving unmodified). The vhost lives in the separate
`vya-ws/nginx` repo. If nginx strips or does not forward it, the limiter
silently falls back to the peer address and you are back to one shared
bucket. Verify from outside:

```sh
# Well under the limit: expect 200s.
curl -s -o /dev/null -w "%{http_code}
" https://freeflight.flyvoyager.net/weather/flightcat

# A burst from one address: expect 200s then 429s with a Retry-After.
for i in $(seq 1 40); do
  curl -s -o /dev/null -w "%{http_code} " https://freeflight.flyvoyager.net/weather/flightcat
done; echo
```

If a second machine gets 429s immediately after the first machine's
burst, the header is not reaching ff-api and everyone is sharing a bucket.

## Chart hashes and sizes (one-off, per pre-0007 cycle)

Cycles published before `ff-storage` migrations 0007/0008 carry no
`chart_catalog.sha256` or `.bytes`. The Android client needs both, and
without them three things stay switched off (DESIGN.md §8):

- chart downloads install **unverified** — the bundle has always had a
  checksum, chart archives did not;
- a chart set can't say what it will **cost** before you start it, so the
  UI has to show "size unknown";
- archives are **not reused across cycles**, so every AIRAC update
  re-downloads the full ~20GB of sectionals, almost all of which are
  byte-identical to the ones already on the device.

A full `ff-etl` re-run would recover two columns by re-fetching and
re-tiling all that imagery through GDAL. The published archives are
already on the server next to the bundle, so `backfill_chart_hashes`
reads the hash and size straight off them instead — pure metadata, no
GDAL, and it edits the bundle **in place**.

It is idempotent: rows that already have both are skipped, so a re-run
costs a catalogue scan rather than re-reading everything. Set
`FF_BACKFILL_FORCE=1` only if published files were replaced without the
catalogue being updated.

There is no checkout on vya2, so build locally and ship the image, the
same way `ship-image.sh` deploys `ff-api`:

```sh
# locally
docker build --provenance=false -f deploy/Dockerfile.chart-hash-backfill   -t ff-chart-hash-backfill:latest .
docker save ff-chart-hash-backfill:latest | gzip   | ssh root@vya2.flyvoyager.net 'gunzip | docker load'
```

Then, on vya2, run it against a **copy** and swap that in when it
succeeds. `ff-api` reopens the bundle per request, so a live swap needs no
restart — and working on a copy keeps the migrations' `ALTER TABLE` off
the file the service is reading, which is the only part of this that takes
an exclusive lock:

```sh
CYC=2026-08-06
cd /containers/freeflight/data/cycles/$CYC
cp cycle.sqlite /containers/freeflight/cycle.sqlite.bak-$(date +%Y%m%d-%H%M%S)
cp cycle.sqlite cycle.sqlite.new          # beside the archives, which the tool reads

# Point the tool at the copy without touching the live latest.json.
mkdir -p /tmp/ffbackfill
python3 -c 'import json;d=json.load(open("/containers/freeflight/data/latest.json"));d["sqlite_path"]="cycles/'$CYC'/cycle.sqlite.new";json.dump(d,open("/tmp/ffbackfill/latest.json","w"))'

# /data is mounted read-write here; ff-api's own mount stays read-only.
docker run --rm   -v /containers/freeflight/data:/data   -v /tmp/ffbackfill/latest.json:/latestdir/latest.json:ro   ff-chart-hash-backfill:latest sh -c '
    mkdir -p /work && cp /latestdir/latest.json /work/latest.json
    ln -s /data/cycles /work/cycles
    FF_ETL_DATA_DIR=/work backfill_chart_hashes'

mv cycle.sqlite.new cycle.sqlite          # atomic, same filesystem
rm -rf /tmp/ffbackfill
```

Expect roughly a minute per 2–3GB of archives — about 6–8 minutes for a
full nationwide cycle (~19GB), since it is reading every byte of every
chart. It logs one line per chart hashed and finishes with
`filled=… skipped=… missing=…`; `missing` counts charts catalogued but
not present on that disk, which is normal for a partial mirror and is not
an error.

Verify before swapping (there is no `sqlite3` binary on vya2, but
`python3` is there):

```sh
python3 -c '
import sqlite3
c = sqlite3.connect("cycle.sqlite.new")
print(c.execute("SELECT COUNT(*), COUNT(sha256), COUNT(bytes) FROM chart_catalog").fetchone())
print([r[0] for r in c.execute("SELECT version FROM schema_migrations ORDER BY version")])
print(c.execute("SELECT COUNT(*) FROM airport").fetchone())'
```

All three counts should match, the migrations should include 7 and 8, and
the airport count should be unchanged — that last one is the check that
you are about to swap in a bundle that is still a bundle.

Run on 2026-08-06 (2026-09-15): `filled=181 skipped=0 missing=0`, about
70 seconds to read 23.9GB of archives.

## Account database (one-time setup)

`ff-api` stores users, sessions, and aircraft records in PostgreSQL
(DESIGN.md §9.5) — the colocated `postgres18` container, which is already
on the `vya2net` network `freeflight-api` joins, so there is nothing to
publish and no `compose.yml` change. Everything here is **optional**:
with `FF_DATABASE_URL` unset, ff-api starts exactly as it does today.

**1. Create the role and database** (matching the `missiongen`/`butterlog`
convention of a dedicated login role owning a same-named database). Use a
password with no characters needing percent-encoding, to keep the URL
readable:

```sh
# openssl rather than `tr -dc ... </dev/urandom | head -c 32`: head exits
# early there, tr dies of SIGPIPE, and under `set -o pipefail` that is a
# fatal 141 rather than a password.
PW=$(openssl rand -hex 24)
printf "CREATE ROLE freeflight LOGIN PASSWORD '%s';\n" "$PW" \
  | docker exec -i postgres18 psql -U postgres -v ON_ERROR_STOP=1
echo "CREATE DATABASE freeflight OWNER freeflight;" \
  | docker exec -i postgres18 psql -U postgres -v ON_ERROR_STOP=1
echo "FF_DATABASE_URL=postgres://freeflight:$PW@postgres18:5432/freeflight"
```

SQL goes in on stdin rather than via `psql -c` so the password never
appears in `ps`. For the same reason, if you script this, do **not**
deliver the script itself on stdin (`ssh root@vya2 'bash -s' <<EOF`) —
`docker exec -i` reads stdin too and will silently eat the rest of your
script mid-run.

Put that line in `/containers/freeflight/.env`. The schema itself is
applied by ff-api on startup (embedded `sqlx` migrations) — there is no
separate migrate step.

> The password must be **percent-encoded** in the URL if it contains
> `@`, `/`, `:` or `#`. It is a URI, parsed by both `sqlx` and the backup
> script's `pg_dump`; an unencoded `@` produces a confusing "could not
> translate host name" error rather than an auth failure.

**2. Install the backup job.** vya2 has **no cluster-wide `pg_dump`** —
`missiongen` and `butterlog` each back up their own database by name, so
a new `freeflight` database is covered by nothing until this is
installed, and the gap is silent. The cycle bundle can always be rebuilt
from FAA/NOAA sources; the aircraft performance numbers a pilot typed out
of their POH cannot.

```sh
install -m 0755 backup-db.sh /containers/freeflight/backup-db.sh
install -m 0644 freeflight-backup.{service,timer} /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now freeflight-backup.timer
systemctl start freeflight-backup.service   # prove it works now
ls -la /containers/freeflight/backups/
```

Restores are `pg_restore` from the dump, e.g. into a scratch database to
inspect before promoting:

```sh
docker exec postgres18 psql -U postgres -c 'CREATE DATABASE ff_restore;'
docker run --rm --network vya2net -v /containers/freeflight/backups:/b \
  -e PGPASSWORD=... postgres:18 \
  pg_restore -h postgres18 -U freeflight -d ff_restore /b/freeflight_YYYYMMDD_HHMMSS.dump
```

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
GDAL is available (WSL/Debian has both; a full run is ~2h40m and ~23 GB),
then ship it.

The build's inputs live in `.env` at the repo root — gitignored, with
`.env.example` tracked beside it as the template. Load it first:

```sh
set -a; . ./.env; set +a      # FF_OPENAIP_API_KEY, FF_AIXM_FR_PATH, FF_ETL_DATA_DIR
```

Both data variables are optional, which is the trap: unset, the pipeline
builds a cycle *without* that country and says so only in a log line. A
US-only cycle is ~13,300 airports where a complete one is ~18,800. Check
the attributions of what you built before publishing it:

```sh
sqlite3 <bundle> 'SELECT name, effective_date FROM data_source'
```

**Copy the cycle directory first and `latest.json` only afterwards.**
`latest.json` is what makes a cycle live, so copying it alongside a
transfer still in flight points clients at a half-present cycle. Both
ends have `rsync`, which is resumable and verifying — the per-file `scp`
loop this used to describe is only needed when a client lacks it.

```sh
# 1. the cycle itself (~23 GB; note the trailing slashes)
rsync -a --partial   ~/cycle-build/cycles/<id>/   root@vya2.flyvoyager.net:/containers/freeflight/data/cycles/<id>/

# 2. confirm it arrived whole before making it live
ssh root@vya2.flyvoyager.net   'du -sh /containers/freeflight/data/cycles/<id>;    ls /containers/freeflight/data/cycles/<id> | wc -l'

# 3. only now the pointer, then pick it up
rsync -a ~/cycle-build/latest.json   root@vya2.flyvoyager.net:/containers/freeflight/data/latest.json
ssh root@vya2.flyvoyager.net   'cd /containers/freeflight && docker compose restart'
```

Rolling back is restoring the previous `latest.json` and restarting —
superseded cycles stay on disk, and installed clients are unaffected
either way, since they only re-read the manifest when checking for an
update.

**3a. Cycle with non-US (France / SIA AIXM) data.** The pipeline folds in
French airports/navaids/waypoints/runways/airways/airspace from the SIA
AIXM 4.5 export when `FF_AIXM_FR_PATH` points at it (DESIGN.md §3.1). This
is opt-in — a normal cycle build (step 3) omits it. To publish an
**up-to-date** France-inclusive cycle:

1. **Matching the AIRAC cycle is preferred, not required.** The FAA and
   SIA both follow the global ICAO AIRAC calendar (28-day, synchronized
   effective dates), so the matching export is the one to use when you
   have it. You no longer have to: the pipeline records the SIA export's
   own effective date and builds a mixed-cycle bundle, and the clients
   flag it wherever they show the cycle date. An export that declares no
   effective date is still refused — nothing downstream could say how old
   it is.

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

4. **Check AIRAC alignment.** In the log, compare `fetched CIFP
   cycle=<date>` against your SIA export's cycle. A mismatch is logged as
   `mixed-cycle bundle: ...` by both the pipeline and validation, and is
   publishable — the clients say so on the cycle card. Re-download the
   matching export when you can; publish the mixed bundle when you can't,
   rather than shipping no France data.

5. **Attribution (Licence Ouverte) — already satisfied.** A France-
   inclusive cycle must display "Service de l'Information Aéronautique
   (SIA)" **and the export's effective date**. Both clients do: web's
   About page renders `AIRAC effective <date>` from `/data/attributions`,
   and Android's Settings lists each credit with `effective <date>`. (An
   earlier note here said the per-cycle date was not wired through; it
   is.)

6. **Publish** exactly as step 3 — cycle directory first, `latest.json`
   last.

**3b. Adding a country to a cycle that is already built.** Re-running the
pipeline to fold in a national export re-fetches CIFP/NASR and re-tiles
all 181 charts: hours of GDAL work to insert a few thousand rows. Use the
standalone tool instead, which writes into the built bundle in place and
also fills in `airac_cycle` if the bundle predates that being recorded:

```sh
FF_AIXM_FR_PATH=/path/to/export_xml_bd_sia_<date>.zip FF_AIXM_TARGET_BUNDLE=~/cycle-build/cycles/<id>/cycle.sqlite FF_AIXM_CYCLE_ID=<id>   cargo run --release -p ff-etl --example add_aixm_to_bundle
```

Safe to do after the bundle is built and even after `latest.json` is
written: `ff-api` hashes `cycle.sqlite` per request (`routes/cycles.rs`),
so the manifest's `sqlite_sha256` follows the file rather than going
stale. The openAIP equivalent is `add_openaip_to_bundle` (needs
`FF_OPENAIP_API_KEY`).

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
