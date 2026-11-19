#!/bin/sh
# Every file that dist-workspace.toml lists in `include` is copied into the release archives, so a
# missing one fails the release job. Licence file changes no longer start the release rehearsal on
# pull requests, so this check keeps the include list honest from the static job instead.
set -eu
cd "$(dirname "$0")/.."

line=$(grep -E '^include[[:space:]]*=' dist-workspace.toml || true)
if [ -z "$line" ]; then
    echo "dist-workspace.toml has no include list" >&2
    exit 1
fi

status=0
for name in $(printf '%s\n' "$line" | grep -oE '"[^"]+"' | tr -d '"'); do
    if [ ! -f "$name" ]; then
        echo "dist-workspace.toml includes $name, but the file does not exist" >&2
        status=1
    fi
done
exit "$status"
