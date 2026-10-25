#!/usr/bin/env bash
# Signs SHA256SUMS and every desktop installer and updater artifact it lists with
# Minisign, then verifies each signature against one of the given public keys and the
# checksums against the files. Tauri updater `.sig` files are covered by SHA256SUMS but
# not signed again.
#
#   MINISIGN_SECRET_KEY=... [MINISIGN_PASSWORD=...] \
#     scripts/desktop-release-sign.sh <dir> <vX.Y.Z> <public-key>...
#
# The trusted comment names the release and file and never carries a `pack:` token, so
# a desktop signature cannot pass as a pack manifest signature made with the same key.
set -euo pipefail

if (($# < 3)); then
  echo 'Usage: desktop-release-sign.sh <dir> <vX.Y.Z> <public-key>...' >&2
  exit 2
fi
dir=$1
tag=$2
shift 2
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Not a stable release tag: $tag" >&2; exit 2; }
[[ -s $dir/SHA256SUMS ]] || { echo "Missing $dir/SHA256SUMS" >&2; exit 1; }
if [[ -z ${MINISIGN_SECRET_KEY:-} ]]; then
  echo 'MINISIGN_SECRET_KEY is empty; refusing to publish unsigned checksums.' >&2
  exit 1
fi

key_dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/desktop-minisign.XXXXXX")
trap 'rm -rf "$key_dir"' EXIT
umask 077
key=$key_dir/release.key
# Accept the key file as written by `minisign -G` or as its bare base64 line.
if [[ $MINISIGN_SECRET_KEY != 'untrusted comment:'* ]]; then
  printf 'untrusted comment: minisign secret key\n' > "$key"
fi
printf '%s\n' "$MINISIGN_SECRET_KEY" >> "$key"

names=()
while read -r digest name; do
  [[ $digest =~ ^[0-9a-f]{64}$ && $name =~ ^[A-Za-z0-9][A-Za-z0-9_.+-]*$ ]] || {
    echo "Malformed SHA256SUMS line for: $name" >&2
    exit 1
  }
  [[ $name == *.sig ]] || names+=("$name")
done < "$dir/SHA256SUMS"
names+=(SHA256SUMS)

for name in "${names[@]}"; do
  # An unencrypted key never reads the password; an encrypted one reads it from stdin
  # and fails on the empty input when no password is configured.
  minisign -S -s "$key" -m "$dir/$name" -t "desktop:AriadShift release:$tag file:$name" \
    >/dev/null <<< "${MINISIGN_PASSWORD:-}"
done

public_key=
for candidate in "$@"; do
  if minisign -V -q -p "$candidate" -m "$dir/SHA256SUMS" >/dev/null 2>&1; then
    public_key=$candidate
    break
  fi
done
if [[ -z $public_key ]]; then
  echo 'The signing key matches none of the given public keys.' >&2
  exit 1
fi
for name in "${names[@]}"; do
  minisign -V -q -p "$public_key" -m "$dir/$name"
done

if command -v sha256sum >/dev/null; then
  (cd "$dir" && sha256sum --check --strict --quiet SHA256SUMS)
else
  (cd "$dir" && shasum -a 256 --check --strict --quiet SHA256SUMS)
fi
echo "Signed and verified ${#names[@]} files in $dir with $(basename "$public_key")."
