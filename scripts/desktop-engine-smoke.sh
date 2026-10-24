#!/usr/bin/env bash
set -euo pipefail

images=(target/release/bundle/appimage/*.AppImage)
if [[ ${#images[@]} != 1 || ! -f "${images[0]}" ]]; then
    echo 'Expected one AppImage; run just desktop-build first.' >&2
    exit 1
fi
image=$(realpath "${images[0]}")
bundle_dir=$(mktemp -d /tmp/ariadshift-appimage-engines.XXXXXX)
trap 'rm -rf "$bundle_dir"' EXIT

# Explicit extraction keeps the bundle alive while the test harness runs.
(cd "$bundle_dir" && "$image" --appimage-extract >/dev/null)
export APPDIR="$bundle_dir/squashfs-root"
export ARIAD_TEST_APPIMAGE_BINARY="$APPDIR/AppRun"
cargo nextest run -p ariad-cli --features ariad-host/test-probe \
    --test builtin_engine_contracts --run-ignored ignored-only --success-output immediate \
    -E 'test(appimage_office_conversion_preserves_the_bundle_in_private_tmp)'
