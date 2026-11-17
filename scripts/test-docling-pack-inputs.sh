#!/usr/bin/env bash
# Checks that PACK_INPUT_HASH moves with each pack input and only with them, and that the script
# fails closed.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/docling-pack-inputs.sh"
workflow="$here/../.github/workflows/pack-docling.yml"
failed=0

pass() { echo "PASS | $1"; }
fail() { echo "FAIL | $1" >&2; failed=1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
git init -q .
git config user.email test@example.test
git config user.name test

mapfile -t inputs < <(bash "$script" --paths)
for path in "${inputs[@]}"; do
    mkdir -p "$(dirname "$path")"
    if [[ "$path" == engines/docling || "$path" == engines/common/licenses ]]; then
        mkdir -p "$path"
        echo "$path" >"$path/file.txt"
    else
        echo "$path" >"$path"
    fi
done
mkdir -p crates
echo base >crates/other.rs
git add . && git commit -q -m base
base_commit=$(git rev-parse HEAD)
base=$(bash "$script" HEAD)

# Edit one file, commit, compare the hash, return to the base commit.
check() {
    # check <same|different> <file> <description>
    echo changed >>"$2"
    git add "$2" && git commit -q -m change
    local now
    now=$(bash "$script" HEAD)
    if [[ "$1" == same && "$now" == "$base" ]] || [[ "$1" == different && "$now" != "$base" ]]; then
        pass "$3"
    else
        fail "$3 (expected $1)"
    fi
    git reset -q --hard "$base_commit"
}

check same crates/other.rs "an unrelated Rust file keeps the hash"
check same README.md "a prose file keeps the hash"
check same Cargo.toml "the workspace manifest keeps the hash"
for path in "${inputs[@]}"; do
    file=$path
    [[ -d "$path" ]] && file="$path/file.txt"
    check different "$file" "$path changes the hash"
done

# An older revision hashes from its tree, with no checkout of it.
echo changed >>"${inputs[0]}/file.txt" 2>/dev/null || echo changed >>"${inputs[0]}"
git add . && git commit -q -m later
if [[ "$(bash "$script" "$base_commit")" == "$base" ]]; then
    pass "an older revision hashes from its tree"
else
    fail "an older revision hashes from its tree"
fi

# Fails closed: unknown revision, and a tree without any input.
if out=$(bash "$script" no-such-revision 2>/dev/null); then
    fail "an unknown revision exits non-zero"
elif [[ -z "$out" ]]; then
    pass "an unknown revision exits non-zero with no output"
else
    fail "an unknown revision prints nothing"
fi
git checkout -q --orphan empty && git rm -rfq . && git commit -q --allow-empty -m empty
if bash "$script" HEAD >/dev/null 2>&1; then
    fail "a tree without inputs exits non-zero"
else
    pass "a tree without inputs exits non-zero"
fi

# The pack workflow's path filters (pull_request and push) must cover every input. Plain
# patterns only: a back-reference is not portable to BSD grep.
for path in "${inputs[@]}"; do
    escaped=${path//./\\.}
    count=$(grep -c -E "^ +- ${escaped}$|^ +- ${escaped}/\*\*$" "$workflow" || true)
    if [[ "$count" -ge 2 ]]; then
        pass "the pack workflow filters pull requests and pushes on $path"
    else
        fail "the pack workflow lists $path $count time(s); pull_request and push both need it"
    fi
done

exit "$failed"
