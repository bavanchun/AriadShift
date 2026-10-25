#!/bin/sh
# Writes the S3 identity file from the environment, then starts SeaweedFS with
# its S3 gateway. The one identity may only read, write, list and tag objects
# in the AriadShift bucket; it cannot create buckets or reach other buckets.
set -eu
: "${S3_BUCKET:?S3_BUCKET is required}"
: "${S3_ACCESS_KEY_ID:?S3_ACCESS_KEY_ID is required}"
: "${S3_SECRET_ACCESS_KEY:?S3_SECRET_ACCESS_KEY is required}"
umask 077
cat > /run/seaweedfs/s3.json <<JSON
{
  "identities": [
    {
      "name": "ariadshift",
      "credentials": [
        { "accessKey": "${S3_ACCESS_KEY_ID}", "secretKey": "${S3_SECRET_ACCESS_KEY}" }
      ],
      "actions": [
        "Read:${S3_BUCKET}",
        "Write:${S3_BUCKET}",
        "List:${S3_BUCKET}",
        "Tagging:${S3_BUCKET}"
      ]
    }
  ]
}
JSON
# -ip=seaweedfs advertises the service name, which resolves on every network
# the container joins. -volume.max=0 sizes the volume count from free disk.
exec weed server \
    -dir=/data \
    -ip=seaweedfs \
    -ip.bind=0.0.0.0 \
    -volume.max=0 \
    -master.volumeSizeLimitMB=1024 \
    -filer.encryptVolumeData \
    -s3 \
    -s3.port=8333 \
    -s3.config=/run/seaweedfs/s3.json
