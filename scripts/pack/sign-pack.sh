#!/usr/bin/env bash
# Signs a merged pack.toml with minisign. Release CI only: the secret key lives in the protected
# `release` environment and never on a developer machine or in the repository.
#
#   sign-pack.sh --name NAME --version VERSION --manifest FILE --public-key FILE
#
# Environment (all required):
#   MINISIGN_SECRET_KEY   the secret key file content
#   MINISIGN_PASSWORD     its password (empty for a key made without one)
#   GITHUB_ACTIONS        must be "true"
#   GITHUB_REF_TYPE       must be "tag"
#   GITHUB_REF_NAME       must be pack-<name>-v<version>
#
# The trusted comment is `pack:<name> version:<version>`, as docs/packs.md requires. The
# signature is verified against the committed public key before the script returns.
set -euo pipefail

die() {
    printf 'sign-pack: %s\n' "$*" >&2
    exit 1
}

name=''
version=''
manifest=''
public_key=''
while [ "$#" -gt 0 ]; do
    case $1 in
    --name | --version | --manifest | --public-key) ;;
    *) die "unknown option '$1'" ;;
    esac
    [ "$#" -ge 2 ] || die "option $1 needs a value"
    case $1 in
    --name) name=$2 ;;
    --version) version=$2 ;;
    --manifest) manifest=$2 ;;
    --public-key) public_key=$2 ;;
    esac
    shift 2
done

[ -n "$name" ] && [ -n "$version" ] && [ -n "$manifest" ] && [ -n "$public_key" ] ||
    die "--name, --version, --manifest and --public-key are required"
[ -f "$manifest" ] || die "manifest '$manifest' is not a file"
[ -f "$public_key" ] || die "public key '$public_key' is not a file"

[ "${GITHUB_ACTIONS:-}" = true ] || die "signing runs only in GitHub Actions"
[ "${GITHUB_REF_TYPE:-}" = tag ] || die "signing runs only for tag builds"
[ "${GITHUB_REF_NAME:-}" = "pack-$name-v$version" ] ||
    die "tag '${GITHUB_REF_NAME:-}' is not pack-$name-v$version"
[ -n "${MINISIGN_SECRET_KEY:-}" ] || die "MINISIGN_SECRET_KEY is not set"
command -v minisign >/dev/null 2>&1 || die "minisign was not found"

# The manifest must describe exactly the pack the tag names.
grep -q "^name = \"$name\"\$" "$manifest" || die "manifest name differs from $name"
grep -q "^version = \"$version\"\$" "$manifest" || die "manifest version differs from $version"

key_dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/minisign.XXXXXX")
cleanup() {
    rm -rf "$key_dir"
}
trap cleanup EXIT
umask 077
printf '%s\n' "$MINISIGN_SECRET_KEY" >"$key_dir/key"

comment="pack:$name version:$version"
printf '%s\n' "${MINISIGN_PASSWORD:-}" |
    minisign -S -s "$key_dir/key" -m "$manifest" -x "$manifest.minisig" -t "$comment"

# Verify with the committed public key and check the trusted comment tokens.
verified=$(minisign -V -p "$public_key" -m "$manifest" -x "$manifest.minisig" 2>&1) ||
    die "the new signature does not verify against $public_key"
case $verified in
*"$comment"*) ;;
*) die "the verified signature does not carry the trusted comment '$comment'" ;;
esac
printf 'sign-pack: signed %s (%s)\n' "$manifest" "$comment" >&2
