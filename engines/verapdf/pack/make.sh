#!/usr/bin/env bash
# Assemble unsigned veraPDF engine packs using system Java runtime.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH='' cd -- "$here/../../.." && pwd)
requires_ashift=${PACK_REQUIRES_ASHIFT:-">=0.3.0, <2.0.0"}

die() { printf 'verapdf-pack: %s\n' "$*" >&2; exit 1; }

slug() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo linux-x64 ;;
        Linux-aarch64|Linux-arm64) echo linux-arm64 ;;
        Darwin-arm64) echo mac-arm64 ;;
        Darwin-x86_64) echo mac-x64 ;;
        *) die 'unsupported host platform' ;;
    esac
    if [ "$(uname -s)" = Linux ] && ldd --version 2>&1 | grep -qi musl; then
        die 'musl is not supported'
    fi
}

triple() {
    case $1 in
        linux-x64) echo x86_64-unknown-linux-gnu ;;
        linux-arm64) echo aarch64-unknown-linux-gnu ;;
        mac-x64) echo x86_64-apple-darwin ;;
        mac-arm64) echo aarch64-apple-darwin ;;
        *) die "unsupported target: $1" ;;
    esac
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "neither sha256sum nor shasum was found"
    fi
}

extract_cli_jar() {
    local installer_zip=$1
    local out_jar=$2
    local work
    work=$(mktemp -d)
    trap 'rm -rf "$work"' RETURN

    unzip -q -j "$installer_zip" "*/verapdf-izpack-installer-1.30.3.jar" -d "$work" || \
        unzip -q -j "$installer_zip" "verapdf-izpack-installer-1.30.3.jar" -d "$work" || \
        die "could not find verapdf-izpack-installer-1.30.3.jar in $installer_zip"

    local installer_jar="$work/verapdf-izpack-installer-1.30.3.jar"
    local expected_installer_sha="fcba8d703b82295a87c18738b3e035c66e3309b42c9e9c361557be7c76b4a163"
    local actual_installer_sha
    actual_installer_sha=$(sha256_of "$installer_jar")
    [ "$actual_installer_sha" = "$expected_installer_sha" ] || die "installer jar SHA-256 mismatch ($actual_installer_sha != $expected_installer_sha)"

    local java_bin="${ASHIFT_JAVA:-${JAVA_HOME:+${JAVA_HOME}/bin/java}}"
    if [ -z "${java_bin:-}" ] || [ ! -x "$java_bin" ]; then
        if command -v java >/dev/null 2>&1; then
            java_bin="java"
        else
            die "java is required to extract veraPDF CLI from installer jar"
        fi
    fi

    cat << 'XML' > "$work/auto-install.xml"
<AutomatedInstallation langpack="eng">
    <com.izforge.izpack.panels.htmlhello.HTMLHelloPanel id="welcome"/>
    <com.izforge.izpack.panels.target.TargetPanel id="install_dir">
        <installpath>OUTPUT_DIR</installpath>
    </com.izforge.izpack.panels.target.TargetPanel>
    <com.izforge.izpack.panels.packs.PacksPanel id="packs">
        <pack index="0" name="veraPDF CLI" selected="true"/>
    </com.izforge.izpack.panels.packs.PacksPanel>
    <com.izforge.izpack.panels.install.InstallPanel id="install"/>
    <com.izforge.izpack.panels.finish.FinishPanel id="finish"/>
</AutomatedInstallation>
XML
    sed -i "s|OUTPUT_DIR|$work/installed|g" "$work/auto-install.xml"

    "$java_bin" -jar "$installer_jar" "$work/auto-install.xml" >/dev/null 2>&1 || \
        die "IzPack headless installation failed"

    local cli_jar="$work/installed/bin/cli-1.30.3.jar"
    [ -f "$cli_jar" ] || die "installed cli-1.30.3.jar not found"

    local expected_cli_sha="19f6edd5c92841a38d9b3b018a555b7ba173b13e4e0d29edecc232e5ba6c1d70"
    local actual_cli_sha
    actual_cli_sha=$(sha256_of "$cli_jar")
    [ "$actual_cli_sha" = "$expected_cli_sha" ] || die "cli.jar SHA-256 mismatch ($actual_cli_sha != $expected_cli_sha)"

    cp "$cli_jar" "$out_jar"
}

case ${1:-} in
    slug) slug ;;
    verify-archive)
        [ "$#" -eq 3 ] || die 'usage: make.sh verify-archive SLUG FILE'
        match=$(awk -v target="$2" '$1 == target { print $4; count++ } END { if (count != 1) exit 1 }' "$here/targets.txt") || die "unknown target '$2'"
        [ -f "$3" ] || die "missing archive '$3'"
        actual=$(sha256_of "$3")
        [ "$actual" = "$match" ] || die "archive SHA-256 mismatch: $actual != $match"
        ;;
    pack)
        if [ "$#" -eq 4 ]; then
            host=$2 installer_zip=$3 ashift=$4 dest=${PACK_OUT:-"$repo/target/packs/verapdf"}
        elif [ "$#" -eq 5 ]; then
            host=$2 installer_zip=$3 ashift=$4 dest=$5
        else
            die 'usage: make.sh pack SLUG VERAPDF_INSTALLER_ZIP ASHIFT [OUT]'
        fi
        [ "$host" = "$(slug)" ] || die 'pack target must match the host'
        [ -f "$installer_zip" ] || die "missing installer archive '$installer_zip'"
        [ -x "$ashift" ] || die "missing ashift binary '$ashift'"

        expected_zip_sha="b512bf24945503a33fc10738ecffc1f2dccaaf8593a448e94ee9ac86a9559a59"
        actual_zip_sha=$(sha256_of "$installer_zip")
        [ "$actual_zip_sha" = "$expected_zip_sha" ] || die "veraPDF installer zip SHA-256 mismatch ($actual_zip_sha != $expected_zip_sha)"

        staging=$(mktemp -d)
        trap 'rm -rf "$staging"' EXIT

        # 1. Extract CLI jar
        mkdir -p "$staging/share/verapdf"
        extract_cli_jar "$installer_zip" "$staging/share/verapdf/cli.jar"

        # 2. Create launcher script
        mkdir -p "$staging/bin"
        cat << 'EOF' > "$staging/bin/verapdf"
#!/usr/bin/env bash
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
JAVA="${ASHIFT_JAVA:-${JAVA_HOME:+${JAVA_HOME}/bin/java}}"
if [ -z "${JAVA:-}" ] || [ ! -x "$JAVA" ]; then
    if command -v java >/dev/null 2>&1; then
        JAVA="java"
    else
        printf 'verapdf: requires Java (>= 11); please install Java or set JAVA_HOME\n' >&2
        exit 5
    fi
fi
exec "$JAVA" \
  -Dfile.encoding=UTF-8 \
  -Djava.io.tmpdir="${TMPDIR:-/tmp}" \
  -XX:+IgnoreUnrecognizedVMOptions \
  --add-exports=java.base/sun.security.pkcs=ALL-UNNAMED \
  -jar "$here/share/verapdf/cli.jar" \
  "$@"
EOF
        chmod +x "$staging/bin/verapdf"

        # 3. Verify all notices
        for notice in THIRD_PARTY_NOTICES.txt apache-LICENSE-2.0.txt jakarta-EDL-1.0.txt jakarta-EPL-2.0.txt rhino-NOTICE.txt verapdf-MPL-2.0.txt; do
            [ -s "$here/notices/$notice" ] || die "notice $notice is missing"
        done

        target_triple=$(triple "$host")
        mkdir -p "$dest-dist"

        bash "$repo/scripts/pack/build-pack.sh" build \
            --name verapdf \
            --version 0.1.0 \
            --triple "$target_triple" \
            --ashift "$ashift" \
            --out "$dest-dist" \
            --description 'veraPDF PDF/A and PDF/UA validation engine' \
            --license 'MPL-2.0 AND Apache-2.0 AND EPL-2.0 AND EDL-1.0 AND BSD-3-Clause AND MIT' \
            --requires-ashift "$requires_ashift" \
            --engine-protocol ariad-engine/2 \
            --entrypoint bin/verapdf \
            --engine "$staging/bin/verapdf" \
            --extra "$staging/share=share" \
            --notices "$here/notices" \
            --unpack-dir "$dest"
        ;;
    merge)
        [ "$#" -eq 3 ] || die 'usage: make.sh merge DIST ASHIFT'
        dist=$2 ashift=$3
        archives=()
        for host in linux-x64 linux-arm64 mac-x64 mac-arm64; do
            target=$(triple "$host")
            archive=$dist/verapdf-0.1.0-$target-engine.tar.zst
            [ -f "$archive" ] || die "missing engine archive for $host: $archive"
            archives+=(--archive "$target=$archive")
        done
        bash "$repo/scripts/pack/build-pack.sh" merge \
            --name verapdf \
            --version 0.1.0 \
            --ashift "$ashift" \
            --out "$dist" \
            --description 'veraPDF PDF/A and PDF/UA validation engine' \
            --license 'MPL-2.0 AND Apache-2.0 AND EPL-2.0 AND EDL-1.0 AND BSD-3-Clause AND MIT' \
            --requires-ashift "$requires_ashift" \
            --engine-protocol ariad-engine/2 \
            --entrypoint bin/verapdf \
            "${archives[@]}"
        ;;
    verify)
        if [ "$#" -eq 1 ]; then
            root=${PACK_OUT:-"$repo/target/packs/verapdf"}
        elif [ "$#" -eq 2 ]; then
            root=$2
        else
            die 'usage: make.sh verify [PACK_DIR]'
        fi
        [ -x "$root/bin/verapdf" ] || die 'verapdf launcher is missing'
        [ -f "$root/share/verapdf/cli.jar" ] || die 'veraPDF cli jar is missing'
        grep -q '^name = "verapdf"$' "$root/pack.toml" || die 'incorrect pack name'
        grep -q '^engine_protocol = "ariad-engine/2"$' "$root/pack.toml" || die 'incorrect engine protocol'
        for notice in THIRD_PARTY_NOTICES.txt apache-LICENSE-2.0.txt jakarta-EDL-1.0.txt jakarta-EPL-2.0.txt rhino-NOTICE.txt verapdf-MPL-2.0.txt; do
            [ -s "$root/notices/$notice" ] || die "notice $notice is missing from pack"
        done
        if grep -riE 'temurin|openjdk|jlink|classpath exception' "$root/notices" >/dev/null 2>&1; then
            die "prohibited JRE terms found in pack notices: $root/notices"
        fi
        ;;
    *) die 'unknown command' ;;
esac
