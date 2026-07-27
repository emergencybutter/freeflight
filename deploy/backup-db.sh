#!/usr/bin/env bash
# Daily backup of freeflight's account database (DESIGN.md §9.5.8).
#
# WHY THIS EXISTS: vya2's Postgres has no cluster-wide dump. Every app
# backs up its own database by name (missiongen via cron, butterlog via a
# systemd timer), so a newly created `freeflight` database is covered by
# nothing at all until this runs — and the gap is silent. Everything else
# freeflight stores can be rebuilt from upstream (FAA/NOAA data, cycle
# bundles); the aircraft performance tables a pilot typed out of a POH
# cannot. This is the only copy.
#
# Shape follows /containers/missiongen/backup-db.sh (pg_dump from a
# throwaway postgres:18 client on vya2net, prune locally, push offsite to
# Scaleway Dedibackup). It lives in the repo rather than only on the
# server so it is reviewable and versioned with the schema it protects.
#
# Auth: if DEDIBACKUP_LOGIN is set in the env file, connect with
# login+password; otherwise fall back to IP-based autologin (user "auto",
# empty password), which only works from the IP the backup space is bound
# to.
#
# Usage:  deploy/backup-db.sh          (as root on vya2, via the timer)
# Env file: /containers/freeflight/.env  — the same file compose.yml
#           loads, so the DB password is defined in exactly one place.
set -euo pipefail

ENV_FILE="${FF_ENV_FILE:-/containers/freeflight/.env}"
LOCAL_DIR="${FF_BACKUP_DIR:-/containers/freeflight/backups}"
KEEP_LOCAL_DAYS="${FF_BACKUP_KEEP_DAYS:-14}"
PG_CONTAINER_NETWORK="${FF_PG_NETWORK:-vya2net}"
STAMP=$(date +%Y%m%d_%H%M%S)
NAME="freeflight_${STAMP}.dump"

[ -r "$ENV_FILE" ] || { echo "env file $ENV_FILE not readable" >&2; exit 1; }
val() { sed -nE "s#^$1=(.*)#\1#p" "$ENV_FILE"; }

# The app's own connection string, used whole — rotating the credential
# in .env rotates it here with nothing else to update, and there is no
# host/user/dbname duplicated between this script and compose.yml to
# drift apart.
#
# Deliberately NOT split into user/password/host the way missiongen's
# script does: that pattern reads the password with a `[^@]+` match,
# which truncates at the first '@' and so breaks on any password
# containing one. Handing the URI to pg_dump avoids parsing it at all.
#
# NOTE: the password inside FF_DATABASE_URL must be percent-encoded
# ('@' -> %40, '/' -> %2F). That is not a rule this script invents —
# sqlx parses the same URL, so an unencoded password breaks ff-api
# itself before it ever gets here.
DB_URL=$(val FF_DATABASE_URL)
[ -n "$DB_URL" ] || { echo "FF_DATABASE_URL not set in $ENV_FILE" >&2; exit 1; }

mkdir -p "$LOCAL_DIR"

# Dump to a .partial name and only rename once it has been checked, so a
# failed or interrupted run can never leave something that looks like a
# good backup sitting in the directory. The trap covers the interrupted
# case; `set -e` covers the failed one.
PARTIAL="$LOCAL_DIR/$NAME.partial"
trap 'rm -f "$PARTIAL"' EXIT

# 1. Dump (custom format, compressed) via a throwaway postgres:18 client
#    on the shared network — there is no host-side psql on vya2. The URL
#    goes in as an env var rather than an argument so the credential does
#    not sit in the container's visible command line.
docker run --rm --network "$PG_CONTAINER_NETWORK" -e PGURL="$DB_URL" \
  -v "$LOCAL_DIR":/backup postgres:18 \
  sh -c 'exec pg_dump -Fc -d "$PGURL" -f "/backup/'"$NAME"'.partial"'

# Verify the archive is actually readable before trusting it. This asks
# the real question ("is this a restorable dump?") rather than a proxy
# one about file size — a legitimately small dump is fine (an empty
# database before the first migration is exactly that), whereas a
# truncated or half-written archive of any size is not.
docker run --rm -v "$LOCAL_DIR":/backup postgres:18 \
  pg_restore -l "/backup/$NAME.partial" >/dev/null \
  || { echo "dump is not a readable pg_restore archive — refusing it" >&2; exit 1; }

mv "$PARTIAL" "$LOCAL_DIR/$NAME"
trap - EXIT
echo "dumped $LOCAL_DIR/$NAME ($(du -h "$LOCAL_DIR/$NAME" | cut -f1))"

# 2. Prune old local dumps (only the auto-named daily ones; leave any
#    manual dumps alone).
find "$LOCAL_DIR" -maxdepth 1 -regextype posix-extended \
  -regex '.*/freeflight_[0-9]{8}_[0-9]{6}\.dump' -mtime +"$KEEP_LOCAL_DAYS" -delete

# 3. Upload offsite to Dedibackup.
LOGIN=$(val DEDIBACKUP_LOGIN)
PASSWORD=$(val DEDIBACKUP_PASSWORD)
HOST=$(val DEDIBACKUP_HOST)
if [ -n "$LOGIN" ]; then CRED="$LOGIN:$PASSWORD"; else CRED="auto:"; fi
if [ -n "$HOST" ]; then HOSTS="$HOST"; else HOSTS="dedibackup-dc3.online.net dedibackup-dc2.online.net"; fi

for H in $HOSTS; do
  # dedibackup-dcX.online.net resolves to several frontend IPs, but the
  # PASV data port only exists on the backend that answered the control
  # connection. curl's default --ftp-skip-pasv-ip re-resolves the
  # hostname for the data connection (may land elsewhere -> "connection
  # refused"); trust the PASV-supplied IP instead, matching lftp.
  if curl -fsS --connect-timeout 20 --no-ftp-skip-pasv-ip --ftp-create-dirs --user "$CRED" \
       -T "$LOCAL_DIR/$NAME" "ftp://$H/freeflight/$NAME"; then
    echo "uploaded to $H:/freeflight/$NAME"
    exit 0
  fi
  echo "upload failed on $H, trying next..." >&2
done

echo "FTP upload failed on all hosts" >&2
echo "local dump kept at $LOCAL_DIR/$NAME" >&2
exit 1
