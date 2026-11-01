#!/bin/sh
# Build the image, smoke-test it, and push it to Docker Hub.
#
#   ./docker/build_and_push.sh            build + test + push
#   ./docker/build_and_push.sh --no-push  build + test only
#
# The test runs on an image built for this machine's architecture. The image
# pushed to Docker Hub is multi-architecture (PLATFORMS, default x86 + ARM):
# Docker picks the right one on each host. The architecture this machine is
# not is built under QEMU emulation, which is much slower.
#   PLATFORMS=linux/arm64 ./docker/build_and_push.sh   push only ARM
#
# Run from a normal terminal where your user can use docker
# (member of the "docker" group after logging in again, or via sudo).
set -e

IMAGE="pajaropinto/axpert_monitor_multi_x64_arm_en"
PLATFORMS="${PLATFORMS:-linux/amd64,linux/arm64}"
BUILDER="axpert-multiarch"
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
# Image tag: major.minor of the Cargo version (2.0.0 → 2.0)
TAG=$(echo "$VERSION" | cut -d. -f1,2)

echo "==> Building ${IMAGE}:${VERSION}, :${TAG} and :latest"
docker build --pull --build-arg VERSION="$VERSION" \
    -t "${IMAGE}:${VERSION}" -t "${IMAGE}:${TAG}" -t "${IMAGE}:latest" .

echo "==> Smoke test"
TMP=$(mktemp -d)
NAME="axpert_smoke_test_$$"
cleanup() {
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    # Files on the test volume belong to root: remove them from a container
    docker run --rm --entrypoint rm -v "$TMP:/t" "${IMAGE}:${TAG}" -rf /t/config /t/data \
        >/dev/null 2>&1 || true
    rm -rf "$TMP"
}
trap cleanup EXIT

docker run -d --name "$NAME" \
    -v "$TMP/config:/app/config" -v "$TMP/data:/app/data" \
    "${IMAGE}:${TAG}" >/dev/null

i=0
until [ "$(docker inspect -f '{{.State.Health.Status}}' "$NAME")" = "healthy" ]; do
    i=$((i + 1))
    # The first health probe runs 30 s after start: allow up to ~90 s
    if [ "$i" -gt 45 ] || [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" != "true" ]; then
        echo "FAILED: container did not become healthy. Healthcheck output:"
        docker inspect -f '{{range .State.Health.Log}}{{.ExitCode}}: {{.Output}}{{"\n"}}{{end}}' "$NAME"
        echo "Container logs (last 20 lines):"
        docker logs --tail 20 "$NAME"
        exit 1
    fi
    sleep 2
done
echo "    container healthy"

docker exec "$NAME" sh -c '
    set -e
    for t in sudo nano apt apk bash curl jq htop nc mosquitto_sub ping dig ip tini; do
        command -v "$t" >/dev/null || { echo "missing tool: $t"; exit 1; }
    done
    echo "    tools present"
    test -f /app/config/app_config.conf && test -f /app/data/axpert.db
    echo "    config and database created on the volume"
'
ls "$TMP/config" "$TMP/data" >/dev/null
echo "    files visible on the host side of the volume"
docker image ls "${IMAGE}:${TAG}" --format '    image size: {{.Size}}'

if [ "$1" = "--no-push" ]; then
    echo "==> Done (not pushed)"
    exit 0
fi

echo "==> Preparing the multi-architecture builder (${PLATFORMS})"
# QEMU emulators for the architectures this machine is not (registered in
# the kernel until the next reboot)
docker run --privileged --rm tonistiigi/binfmt --install all >/dev/null
if ! docker buildx inspect "$BUILDER" >/dev/null 2>&1; then
    docker buildx create --name "$BUILDER" --driver docker-container >/dev/null
fi
docker buildx inspect "$BUILDER" --bootstrap >/dev/null

if ! docker login >/dev/null 2>&1 </dev/null; then
    echo "==> Logging in to Docker Hub (use an access token as password)"
    docker login
fi

echo "==> Building ${PLATFORMS} and pushing ${IMAGE}:${VERSION} and :latest"
docker buildx build --builder "$BUILDER" --platform "$PLATFORMS" --pull --push \
    --build-arg VERSION="$VERSION" \
    -t "${IMAGE}:${VERSION}" -t "${IMAGE}:latest" .

echo "==> Published ${IMAGE}:${VERSION} and :latest"
docker buildx imagetools inspect "${IMAGE}:${VERSION}" | grep -i 'platform' | sort -u
