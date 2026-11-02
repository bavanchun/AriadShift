#!/usr/bin/env bash
# Record installed package metadata and the bytes of their installed regular files.
set -euo pipefail
out=$1
mkdir -p "$out"
gcc --version > "$out/gcc-version.txt"
sha256sum "$(gcc -print-file-name=libgcc.a)" "$(g++ -print-file-name=libstdc++.a)" > "$out/runtime-sha256.txt"
if command -v rpm >/dev/null; then
    rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | LC_ALL=C sort > "$out/packages.txt"
    : > "$out/installed-files.sha256"
    while IFS= read -r package; do
        query=${package%.(none)}
        rpm -ql "$query" | LC_ALL=C sort | while IFS= read -r file; do
            if [ -f "$file" ] && [ ! -L "$file" ]; then printf '%s\0' "$file"; fi
        done | xargs -0 -r sha256sum > "$out/files.tmp"
        sed "s|^|$package  |" "$out/files.tmp" >> "$out/installed-files.sha256"
        printf '%s  %s\n' "$(sha256sum "$out/files.tmp" | cut -d ' ' -f 1)" "$package"
    done < "$out/packages.txt" > "$out/package-sha256.txt"
else
    apk info -v | LC_ALL=C sort > "$out/packages.txt"
    find "$out/apks" -name '*.apk' -type f -exec sha256sum {} + | sed "s|$out/apks/||" | LC_ALL=C sort > "$out/package-sha256.txt"
    [ "$(wc -l < "$out/packages.txt")" -eq "$(wc -l < "$out/package-sha256.txt")" ]
    rm -rf "$out/apks"
fi
rm -f "$out/files.tmp"
