#!/usr/bin/env sh
# Deploy an ff-api code change by building the image LOCALLY and shipping the
# image itself to vya2 — not the source. Replaces the old "scp a git-archive
# tarball and `docker build` on the server (~20 min cold)" flow: the local
# build is quick (see the committed .dockerignore, which keeps the ~19 GB
# data/ cycle out of the build context) and `docker save | ssh docker load`
# moves only ~33 MB gzipped.
#
# There is no registry (compose uses `pull_policy: never`), so the transfer is
# a plain `docker save` piped over SSH into `docker load`. Both ends are
# linux/amd64. `--provenance=false` keeps the build a single plain manifest so
# `docker save`/`load` round-trips cleanly across image stores.
#
# Usage:  sh deploy/ship-image.sh
# Env:    FF_DEPLOY_SERVER (default root@vya2.flyvoyager.net)
set -eu

SERVER="${FF_DEPLOY_SERVER:-root@vya2.flyvoyager.net}"
IMAGE="ff-api:latest"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> building $IMAGE locally"
docker build --provenance=false -f "$REPO_ROOT/deploy/Dockerfile" -t "$IMAGE" "$REPO_ROOT"

echo "==> shipping image to $SERVER (docker save | ssh docker load)"
docker save "$IMAGE" | gzip | ssh "$SERVER" 'gunzip | docker load'

echo "==> recreating container (compose picks up the new image id)"
ssh "$SERVER" 'cd /containers/freeflight && docker compose up -d'

echo "==> health check"
curl -fsS https://freeflight.flyvoyager.net/health && echo
echo "done. (web/nginx/data changes are separate — see deploy/README.md)"
