#!/usr/bin/env bash
# Check the dynamic dependencies reported by readelf on stdin.
set -euo pipefail
arch=${1:?architecture required}
awk -v arch="$arch" '
    /\(NEEDED\)/ {
        if ($0 ~ /\[(libc\.so\.6|libm\.so\.6|libpthread\.so\.0|libdl\.so\.2|libstdc\+\+\.so\.6|libgcc_s\.so\.1)\]/) next
        if (arch == "aarch64" && $0 ~ /\[ld-linux-aarch64\.so\.1\]/) next
        exit 1
    }
'
