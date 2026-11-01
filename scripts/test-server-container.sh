#!/usr/bin/env bash
# Run from the repository root with the dedicated test PostgreSQL available.
set -euo pipefail
: "${ARIAD_TEST_DATABASE_URL:?set the dedicated test PostgreSQL URL}"
image=ariadshift-server-smoke
api=ariadshift-smoke-api
worker=ariadshift-smoke-worker
port="${ARIAD_CONTAINER_SMOKE_PORT:-28042}"
for container in "$api" "$worker"; do
    if docker inspect "$container" >/dev/null 2>&1; then
        echo "refusing to replace existing container $container" >&2
        exit 1
    fi
done
ARIAD_CONTAINER_SMOKE_PORT="$port" python3 - <<'PY'
import os, socket
port = int(os.environ["ARIAD_CONTAINER_SMOKE_PORT"])
with socket.socket() as listener:
    listener.bind(("127.0.0.1", port))
PY
docker build --check -f infra/docker/server.Dockerfile .
docker build -f infra/docker/server.Dockerfile -t "$image" .
for dockerfile in worker-office worker-docling; do
    docker build --check -f "infra/docker/$dockerfile.Dockerfile" --build-arg "SERVER_IMAGE=$image" .
done
work=$(mktemp -d)
mkdir "$work/objects"
chmod 755 "$work"
chmod 777 "$work/objects"
cleanup() {
    result=$?
    if [ "$result" != 0 ]; then
        docker logs --tail 100 "$api" 2>/dev/null || true
        docker logs --tail 100 "$worker" 2>/dev/null || true
    fi
    docker stop --time 10 "$worker" "$api" >/dev/null 2>&1 || true
    docker rm "$worker" "$api" >/dev/null 2>&1 || true
    # All object files belong to the non-root image user. Let that same user
    # remove them before deleting the owned temporary directory on the host.
    docker run --rm --user 10001 --entrypoint sh -v "$work/objects:/objects" "$image" -c 'rm -rf /objects/*' || true
    rm -rf "$work"
    exit "$result"
}
trap cleanup EXIT
args=(--network host --read-only --cap-drop ALL --cap-add SETUID --cap-add SETGID
    --security-opt no-new-privileges --memory 1g
    --tmpfs /tmp:uid=10001,gid=10001,mode=700,size=64m
    --tmpfs /work:uid=10001,gid=10001,mode=700,size=128m
    -v "$work/objects:/objects"
    -e "DATABASE_URL=$ARIAD_TEST_DATABASE_URL"
    -e ARIAD_STORAGE_URL=file:///objects)
docker run --rm "${args[@]}" "$image" migrate
docker run -d --name "$api" "${args[@]}" \
    -e ARIAD_SECRET=container-smoke-secret-0123456789abcdef \
    -e "ARIAD_BIND=127.0.0.1:$port" -e "ARIAD_PUBLIC_URL=http://127.0.0.1:$port" \
    "$image" api >/dev/null
docker run -d --name "$worker" "${args[@]}" \
    -e ARIAD_WORK_DIR=/work -e ARIAD_SANDBOX=required -e ARIAD_WORKER_POLL_S=1 \
    "$image" worker --packs core >/dev/null
ARIAD_CONTAINER_SMOKE_PORT="$port" python3 - <<'PY'
import hashlib, io, json, os, time, urllib.request, zipfile

port = int(os.environ["ARIAD_CONTAINER_SMOKE_PORT"])
base = f"http://127.0.0.1:{port}"
def call(path, body=None):
    data = None if body is None else json.dumps(body).encode()
    with urllib.request.urlopen(urllib.request.Request(base + path, data=data, headers={"Content-Type": "application/json"}), timeout=5) as response:
        return json.load(response)

for attempt in range(60):
    try:
        call("/readyz")
        break
    except (OSError, ValueError):
        time.sleep(1)
else:
    raise SystemExit("container API never became ready")
data = b"# Container smoke\n\nA real Pandoc conversion.\n"
upload = call("/v1/uploads", {"filename": "smoke.md", "size": len(data), "sha256": hashlib.sha256(data).hexdigest()})
put = upload["upload"]
with urllib.request.urlopen(urllib.request.Request(put["url"], data=data, method=put["method"], headers=put["headers"]), timeout=5):
    pass
call("/v1/uploads/" + upload["id"] + "/complete", {})
job = call("/v1/jobs", {"upload_id": upload["id"], "to": "docx"})
for attempt in range(60):
    job = call("/v1/jobs/" + job["id"])
    if job["status"] in ("succeeded", "failed", "canceled", "expired"):
        break
    time.sleep(1)
assert job["status"] == "succeeded", job
outputs = call("/v1/jobs/" + job["id"] + "/outputs")
with urllib.request.urlopen(outputs["outputs"][0]["url"], timeout=5) as response:
    with zipfile.ZipFile(io.BytesIO(response.read())) as document:
        assert b"Container smoke" in document.read("word/document.xml")
print("container required-mode Markdown to DOCX Pandoc conversion passed")
PY
# Simulate a compromised same-user helper with a cleared environment. It must
# not recover credentials from either the worker or the root tini supervisor.
docker exec --user 10001 "$worker" env -i PATH=/usr/bin:/bin sh -c '
    found=0
    for path in /proc/[0-9]*/comm; do
        IFS= read -r name < "$path" || continue
        [ "$name" = ariad-server ] || continue
        status=${path%comm}status
        grep -Eq "^Uid:[[:space:]]+10001[[:space:]]+10001[[:space:]]+10001[[:space:]]+10001$" "$status" || exit 1
        for field in CapInh CapPrm CapEff CapAmb; do
            grep -Eq "^$field:[[:space:]]+0000000000000000$" "$status" || exit 1
        done
        grep -Eq "^NoNewPrivs:[[:space:]]+1$" "$status" || exit 1
        found=1
    done
    [ "$found" = 1 ] || exit 1
    for path in /proc/[0-9]*/environ; do
        if cat "$path" 2>/dev/null | tr "\000" "\n" | grep -q "^DATABASE_URL="; then
            echo "worker credentials were readable through procfs" >&2
            exit 1
        fi
    done'
echo "container worker and supervisor procfs protection passed"
