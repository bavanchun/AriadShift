#!/bin/sh
# Unit test for scripts/winget-manifest.sh
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
winget_script="$repo_root/scripts/winget-manifest.sh"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

dummy_zip="$tmpdir/dummy.zip"
echo "dummy archive content" > "$dummy_zip"
expected_sha=$(sha256sum "$dummy_zip" | awk '{print $1}')

echo "=== Test 1: No arguments should fail ==="
if "$winget_script" >/dev/null 2>&1; then
  echo "FAIL: Expected failure on missing arguments" >&2
  exit 1
fi
echo "PASS"

echo "=== Test 2: Invalid version formats should fail ==="
for bad_ver in "v0.1.0" "0.1" "0.1.0-alpha" "0.1&x" "0.1/0" "abc" ""; do
  if "$winget_script" "$bad_ver" "$dummy_zip" "$tmpdir/out" >/dev/null 2>&1; then
    echo "FAIL: Expected failure on invalid version '$bad_ver'" >&2
    exit 1
  fi
done
echo "PASS"

echo "=== Test 2b: Version with newline should fail and leave no 0-byte files ==="
bad_newline="$(printf '0.1.0\nX')"
out_newline="$tmpdir/out_newline"
err_newline="$tmpdir/err_newline.txt"
if "$winget_script" "$bad_newline" "$dummy_zip" "$out_newline" >/dev/null 2>"$err_newline"; then
  echo "FAIL: Expected failure on version with newline" >&2
  exit 1
fi
if ! grep -q "Version must not contain newlines" "$err_newline"; then
  echo "FAIL: Expected specific newline error message, got:" >&2
  cat "$err_newline" >&2
  exit 1
fi
if [ -d "$out_newline" ] && [ "$(find "$out_newline" -type f -size 0 | wc -l)" -gt 0 ]; then
  echo "FAIL: 0-byte files left in output directory on failure" >&2
  exit 1
fi
echo "PASS"

echo "=== Test 3: Nonexistent archive path should fail ==="
if "$winget_script" "0.1.0" "$tmpdir/nonexistent.zip" "$tmpdir/out" >/dev/null 2>&1; then
  echo "FAIL: Expected failure on nonexistent archive" >&2
  exit 1
fi
echo "PASS"

echo "=== Test 4: Missing archive and no online asset should fail ==="
if "$winget_script" "999.999.999" "" "$tmpdir/out" >/dev/null 2>&1; then
  echo "FAIL: Expected failure when archive and online asset are absent" >&2
  exit 1
fi
echo "PASS"

echo "=== Test 5: Valid version and local archive should succeed ==="
out_dir="$tmpdir/out_valid"
"$winget_script" "0.1.0" "$dummy_zip" "$out_dir" >/dev/null

for file in "VChun.AriadShift.yaml" "VChun.AriadShift.installer.yaml" "VChun.AriadShift.locale.en-US.yaml"; do
  if [ ! -f "$out_dir/$file" ]; then
    echo "FAIL: Missing generated file $out_dir/$file" >&2
    exit 1
  fi
done

# Verify version substitution
if ! grep -q "PackageVersion: 0.1.0" "$out_dir/VChun.AriadShift.yaml"; then
  echo "FAIL: PackageVersion 0.1.0 not found in version manifest" >&2
  exit 1
fi

# Verify hash substitution
if ! grep -q "$expected_sha" "$out_dir/VChun.AriadShift.installer.yaml"; then
  echo "FAIL: Expected SHA $expected_sha not found in installer manifest" >&2
  exit 1
fi
echo "PASS"

echo "All winget-manifest tests passed."
