#!/bin/sh
# Decides, inside GitHub Actions, which commits to lint, whether the
# multi-OS build and test jobs must run, whether the packaging check must run,
# and whether the web job must run. See docs/ci.md for the rules.
#
# Input (environment): EVENT_NAME, PR_BASE_SHA, PR_HEAD_SHA, PR_BASE_REF,
# PUSH_BEFORE, REF_NAME, GITHUB_REPOSITORY, GH_TOKEN.
# Arguments: --classify <path>... or --classify with paths on stdin.
# Output (GITHUB_OUTPUT): range, heavy, web, package, linux_parts, reason.
# --classify-cloud [paths...] prints true if any path needs the cloud E2E.
# --classify-package [--added <path>]... [paths...] prints true if the change can
# affect `cargo package`. An --added path counts as changed and as added.
set -eu

zero='0000000000000000000000000000000000000000'
range=''
target_ref=''
changed=''

# Prose the static job already spell-checks.
docs_paths='^(docs|plans)/|^(README|ARCHITECTURE|AGENTS|CLAUDE)\.md$'
# The JavaScript workspace (apps, packages, Node services) and the brand assets the
# site copies; the Rust jobs read none of them.
js_paths='^(apps|packages|services|brand)/|^(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|biome\.json|vitest\.config\.ts|\.node-version)$|^scripts/check-js-licenses\.mjs$'
# The compose end-to-end test and its workflow; the static job lints the workflow, and
# the cloud-e2e workflow runs the test, so neither needs the multi-OS Rust jobs.
e2e_paths='^infra/test/e2e/|^\.github/workflows/cloud-e2e\.yml$'
# The pdfium engine pack scripts and workflow; the pack-pdfium workflow tests and builds them,
# so the multi-OS Rust jobs have nothing to check.
pack_paths='^(engines/(pdfium|qpdf|tesseract)/|scripts/pack/)|^scripts/test-pack-(qpdf|tesseract)\.sh$|^\.github/workflows/pack-(pdfium|qpdf|tesseract)\.yml$'
# Everything the web job builds or depends on, including the Rust code compiled to wasm
# and the OpenAPI document the SDK is generated from.
web_paths='^(apps|packages|services|brand)/|^crates/ariad-(core|pdf|engine-io|wasm|typst)/|^(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|biome\.json|vitest\.config\.ts|\.node-version|Cargo\.toml|Cargo\.lock|rust-toolchain\.toml|justfile)$|^scripts/(ci-scope\.sh|install-wasm-bindgen\.sh)$|^\.github/workflows/ci\.yml$|^schemas/openapi\.v1\.json$'

# One path list for cloud push/PR scope; cloud-e2e.yml calls this classifier.
cloud_paths='^(infra/|apps/web/|crates/ariad-(server|core|pdf|engine-io|host|cli|wasm|typst)/|services/|packages/|brand/)|^(Cargo\.(toml|lock)|rust-toolchain\.toml|pnpm-lock\.yaml|pnpm-workspace\.yaml|package\.json|\.node-version|\.env\.example|justfile)$|^scripts/(ci-scope\.sh|test-ci-scope\.sh|docling-pack-inputs\.sh|install-wasm-bindgen\.sh)$|^fixtures/(pdf/vi-garden-image-only\.pdf|manifest\.toml)$|^\.github/workflows/(cloud-e2e|pack-docling)\.yml$'

# What `cargo package` (just package-check) reads besides crate source: the workspace manifest,
# lockfile and toolchain, the recipe, the scope rules and the workflow that run it, each crate's
# build script, and the root README.md that every crate manifest names as its readme. Crate
# manifests and every other file under crates/ that is not Rust source (package include lists,
# bundled assets and fixtures) are matched by package_decision, as is any file added under crates/:
# a new file can fall outside a crate's include list, or reach above the crate directory, and
# then only a package build notices. Accepted risk: a plain edit of an existing .rs file cannot
# break packaging by itself, and every push to dev or main runs the check anyway.
package_paths='^(Cargo\.(toml|lock)|rust-toolchain\.toml|justfile|README\.md)$|^crates/[^/]+/build\.rs$|^scripts/ci-scope\.sh$|^\.github/workflows/ci\.yml$'

# Parts of the Linux Verify job, as JSON for its matrix. The test shards carry their own index
# and count, so the workflow never repeats the number of shards.
test_shards=2

# Prints true or false: can the change affect the packaging check?
# $1: every path the change touched (a rename lists both sides), empty when unknown.
# $2: the paths the change added.
package_decision() {
    if [ -z "$1" ]; then
        echo true
    elif printf '%s\n' "$1" | grep -E "$package_paths" >/dev/null; then
        echo true
    elif printf '%s\n' "$1" | grep -E '^crates/' | grep -v -E '\.rs$' >/dev/null; then
        echo true
    elif printf '%s\n' "$2" | grep -E '^crates/' >/dev/null; then
        echo true
    else
        echo false
    fi
}

# Prints the matrix of the Linux Verify job. $1: true when the packaging part must run.
linux_parts_json() {
    parts='{"part":"lint"}'
    shard=1
    while [ "$shard" -le "$test_shards" ]; do
        parts="$parts,{\"part\":\"test-$shard\",\"shard\":$shard,\"shards\":$test_shards}"
        shard=$((shard + 1))
    done
    if [ "$1" = true ]; then
        parts="$parts,{\"part\":\"package\"}"
    fi
    printf '[%s]\n' "$parts"
}

if [ "${1:-}" = --classify-cloud ]; then
    shift
    if [ "$#" -gt 0 ]; then changed=$(printf '%s\n' "$@"); else changed=$(cat); fi
    if [ -z "$changed" ] || printf '%s\n' "$changed" | grep -E "$cloud_paths" >/dev/null; then
        echo true
    else
        echo false
    fi
    exit 0
fi

if [ "${1:-}" = --classify-package ]; then
    shift
    added=''
    while [ "${1:-}" = --added ]; do
        [ "$#" -ge 2 ] || { echo "ci-scope: --added needs a path" >&2; exit 2; }
        added=$(printf '%s\n%s' "$added" "$2")
        shift 2
    done
    if [ "$#" -gt 0 ]; then
        changed=$(printf '%s\n' "$@")
    elif [ -z "$added" ]; then
        changed=$(cat)
    fi
    package_decision "$(printf '%s\n%s' "$changed" "$added" | grep -v '^$' || true)" "$added"
    exit 0
fi

# Prints heavy|web|reason.
decide() {
    # Manual runs and pushes without a known previous commit always run everything.
    if [ "${EVENT_NAME:-}" = workflow_dispatch ] || [ -z "$changed" ]; then
        echo "true|true|full run"
        return
    fi

    # Prose-only changes: the static job already spell-checks them.
    if ! printf '%s\n' "$changed" | grep -v -E "$docs_paths" >/dev/null; then
        echo "false|false|documentation-only change"
        return
    fi

    # Changes into main are promotions or hotfixes. A promotion carries a tree
    # that a push to dev has already verified; find it and skip the rebuild.
    if [ "$target_ref" = main ]; then
        tree=$(git rev-parse 'HEAD^{tree}')
        if shas=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?branch=dev&event=push&status=success&per_page=30" \
            --jq '.workflow_runs[].head_sha' 2>/dev/null); then
            for sha in $shas; do
                if [ "$(git rev-parse "$sha^{tree}" 2>/dev/null || true)" = "$tree" ]; then
                    echo "false|false|tree already verified on dev at $sha"
                    return
                fi
            done
        else
            echo "true|true|could not list dev runs; running everything"
            return
        fi
    fi

    heavy=false
    if printf '%s\n' "$changed" | grep -E '^apps/desktop/src-tauri/' >/dev/null; then
        heavy=true
    fi
    if printf '%s\n' "$changed" | grep -v -E "$docs_paths|$js_paths|$e2e_paths|$pack_paths" >/dev/null; then
        heavy=true
    fi
    web=false
    if printf '%s\n' "$changed" | grep -E "$web_paths" >/dev/null; then
        web=true
    fi
    if [ "$heavy" = true ]; then
        echo "$heavy|$web|code change"
    elif ! printf '%s\n' "$changed" | grep -v -E "$docs_paths|$pack_paths" >/dev/null; then
        echo "$heavy|$web|pack-only change"
    else
        echo "$heavy|$web|JavaScript-only change"
    fi
}

if [ "${1:-}" = "--classify" ]; then
    shift
    if [ "$#" -gt 0 ]; then
        changed=$(printf '%s\n' "$@")
    else
        changed=$(cat)
    fi
    EVENT_NAME="pull_request"
    target_ref="dev"
    decide
    exit 0
fi

case "${EVENT_NAME:-}" in
    pull_request)
        range="${PR_BASE_SHA:-}..${PR_HEAD_SHA:-}"
        changed=$(git diff --name-only "${PR_BASE_SHA:?}"..."${PR_HEAD_SHA:?}")
        target_ref=${PR_BASE_REF:-}
        # Without rename detection a move shows as a delete plus an add, so the packaging
        # check sees the file that left a crate as well as the one that arrived.
        package_changed=$(git diff --name-only --no-renames "$PR_BASE_SHA"..."$PR_HEAD_SHA")
        added=$(git diff --name-only --no-renames --diff-filter=A "$PR_BASE_SHA"..."$PR_HEAD_SHA")
        ;;
    push)
        if [ "${PUSH_BEFORE:-$zero}" != "$zero" ] && git cat-file -e "$PUSH_BEFORE^{commit}" 2>/dev/null; then
            range="$PUSH_BEFORE..HEAD"
            changed=$(git diff --name-only "$PUSH_BEFORE" HEAD)
        fi
        target_ref=${REF_NAME:-}
        ;;
esac

decision=$(decide)
heavy=${decision%%|*}
rest=${decision#*|}
web=${rest%%|*}
reason=${rest#*|}

# The packaging check runs on every manual run and every push, where it also keeps the cache
# warm, and on a pull request only when the change can affect it. It never runs when the heavy
# jobs are skipped.
if [ "$heavy" != true ]; then
    package=false
elif [ "${EVENT_NAME:-}" = workflow_dispatch ] || [ "${EVENT_NAME:-}" = push ]; then
    package=true
else
    package=$(package_decision "${package_changed:-}" "${added:-}")
fi
linux_parts=$(linux_parts_json "$package")

if [ -n "${GITHUB_OUTPUT:-}" ]; then
    {
        printf 'range=%s\n' "$range"
        printf 'heavy=%s\n' "$heavy"
        printf 'web=%s\n' "$web"
        printf 'package=%s\n' "$package"
        printf 'linux_parts=%s\n' "$linux_parts"
        printf 'reason=%s\n' "$reason"
    } >> "$GITHUB_OUTPUT"
fi

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
        printf '### CI scope\n\n'
        printf -- '- Commits checked: `%s`\n' "${range:-none}"
        printf -- '- Build and test on three OSes: **%s** (%s)\n' "$heavy" "$reason"
        printf -- '- Packaging check: **%s**\n' "$package"
        printf -- '- Web checks: **%s**\n' "$web"
    } >> "$GITHUB_STEP_SUMMARY"
fi

printf 'heavy=%s web=%s package=%s (%s), range=%s\n' "$heavy" "$web" "$package" "$reason" "${range:-none}"
