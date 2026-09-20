#!/usr/bin/env bash
# Baut das Praxis-VPS-Image (Binary + gebündeltes POML-CLI).
# Usage: IMAGE_TAG=vayayo/praxis:latest bash deploy/build.sh [--push]
set -euo pipefail
cd "$(dirname "$0")/.."

: "${IMAGE_TAG:=vayayo/praxis:latest}"
: "${POML_DIR:=../poml}"

if [ ! -f "$POML_DIR/package.json" ] || [ ! -f "$POML_DIR/webpack.config.cli.js" ]; then
  echo "POML-Checkout fehlt: $POML_DIR" >&2
  echo "git clone ssh://git@forgejo.the.grid/Marvin/poml.git $POML_DIR" >&2
  exit 1
fi

docker build -f deploy/Dockerfile \
  --build-context "poml=$POML_DIR" \
  -t "$IMAGE_TAG" .
echo "Image $IMAGE_TAG gebaut (praxis + node + POML-CLI-Bundle)."

if [ "${1:-}" = "--push" ]; then
  docker push "$IMAGE_TAG"
fi
