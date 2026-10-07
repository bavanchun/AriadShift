#!/bin/sh
# Generates WinGet manifests for AriadShift from packaging templates.
# Usage: scripts/winget-manifest.sh <version> [archive_path] [output_dir]
set -eu

if [ $# -lt 1 ]; then
  echo "Usage: $0 <version> [archive_path] [output_dir]" >&2
  exit 1
fi

VERSION="$1"
ARCHIVE_PATH="${2:-}"
OUTPUT_DIR="${3:-}"

# Reject any version containing newlines
NL='
'
case "$VERSION" in
  *"$NL"*)
    echo "ERROR: Invalid version format. Version must not contain newlines." >&2
    exit 1
    ;;
esac
if [ "$(printf '%s' "$VERSION" | wc -l)" -ne 0 ]; then
  echo "ERROR: Invalid version format. Version must not contain newlines." >&2
  exit 1
fi

# Strict SemVer validation (X.Y.Z)
if ! printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "ERROR: Invalid version format '$VERSION'. Must match '^[0-9]+\\.[0-9]+\\.[0-9]+$' (e.g. 0.1.0)." >&2
  exit 1
fi

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
tmpl_dir="$repo_root/packaging/winget"

if [ ! -d "$tmpl_dir" ]; then
  echo "ERROR: Template directory not found at $tmpl_dir" >&2
  exit 1
fi

# Determine SHA256 of the Windows zip archive
SHA256=""
if [ -n "$ARCHIVE_PATH" ]; then
  if [ ! -f "$ARCHIVE_PATH" ]; then
    echo "ERROR: Specified archive path does not exist: $ARCHIVE_PATH" >&2
    exit 1
  fi
  SHA256=$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')
else
  # Attempt to fetch published checksum from GitHub Releases
  sha_url="https://github.com/bavanchun/AriadShift/releases/download/v${VERSION}/ariad-cli-x86_64-pc-windows-msvc.zip.sha256"
  echo "Fetching published SHA-256 from $sha_url..."
  downloaded_sha=$(curl --proto '=https' --tlsv1.2 -LsSf "$sha_url" 2>/dev/null | awk '{print $1}' || true)
  if [ -n "$downloaded_sha" ]; then
    SHA256="$downloaded_sha"
  fi
fi

# Validate 64-hex character hash
if [ -z "$SHA256" ] || ! printf '%s\n' "$SHA256" | grep -Eq '^[0-9a-fA-F]{64}$'; then
  echo "ERROR: Could not obtain a valid 64-character hex SHA-256 for version $VERSION." >&2
  echo "Provide a valid archive path argument or ensure the release asset is published." >&2
  exit 1
fi

# Resolve default output directory against repository root
if [ -z "$OUTPUT_DIR" ]; then
  OUTPUT_DIR="$repo_root/target/winget/$VERSION"
else
  case "$OUTPUT_DIR" in
    /*) ;; # already absolute
    *) OUTPUT_DIR="$repo_root/$OUTPUT_DIR" ;;
  esac
fi

mkdir -p "$OUTPUT_DIR"

tmp_stage_dir=$(mktemp -d)
trap 'rm -rf "$tmp_stage_dir"' EXIT

for tmpl in "$tmpl_dir"/*.yaml.tmpl; do
  [ -f "$tmpl" ] || continue
  fname=$(basename "$tmpl" .tmpl)
  tmp_file="$tmp_stage_dir/$fname"
  dest="$OUTPUT_DIR/$fname"

  sed \
    -e "s|{{VERSION}}|$VERSION|g" \
    -e "s|{{SHA256}}|$SHA256|g" \
    "$tmpl" > "$tmp_file"

  mv "$tmp_file" "$dest"
  echo "Generated: $dest"
done

echo "OK: WinGet manifests generated successfully in $OUTPUT_DIR"
