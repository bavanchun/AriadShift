#!/bin/sh
# Creates the AriadShift bucket and a one-day expiry backstop for its t/
# objects. Safe to run on every start.
set -eu
: "${S3_BUCKET:?S3_BUCKET is required}"

# Runs shell commands, one per line. Output may carry "> " prompts.
weed_shell() {
    printf '%s\n' "$@" | weed shell -master=seaweedfs:9333 -filer=seaweedfs:8888 2>&1
}

has_bucket() {
    weed_shell "s3.bucket.list" | grep -Eq "(^|[[:space:]>])${S3_BUCKET}([[:space:]]|$)"
}

for _ in $(seq 1 60); do
    if has_bucket; then
        break
    fi
    weed_shell "s3.bucket.create -name ${S3_BUCKET}" >/dev/null || true
    sleep 2
done
if ! has_bucket; then
    echo "bucket ${S3_BUCKET} was not created" >&2
    exit 1
fi

# The API's retention sweeper deletes objects on time; this TTL only removes
# objects the sweeper missed.
ttl=$(weed_shell "lock" "fs.configure -locationPrefix=/buckets/${S3_BUCKET}/t/ -ttl=1d -apply" "unlock")
if ! printf '%s\n' "$ttl" | grep -q '1d'; then
    printf '%s\n' "$ttl" >&2
    echo "could not set the expiry backstop on ${S3_BUCKET}/t/" >&2
    exit 1
fi
echo "bucket ${S3_BUCKET} is ready"
