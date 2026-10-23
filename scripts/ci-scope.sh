#!/bin/sh
# Decides, inside GitHub Actions, which commits to lint, whether the
# multi-OS build and test jobs must run, and whether the web job must run.
# See docs/ci.md for the rules.
#
# Input (environment): EVENT_NAME, PR_BASE_SHA, PR_HEAD_SHA, PR_BASE_REF,
# PUSH_BEFORE, REF_NAME, GITHUB_REPOSITORY, GH_TOKEN.
# Arguments: --classify <path>... or --classify with paths on stdin.
# Output (GITHUB_OUTPUT): range, heavy, web, reason.
set -eu

zero='0000000000000000000000000000000000000000'
range=''
target_ref=''
changed=''

# Prose the static job already spell-checks.
docs_paths='^(docs|plans)/|^(README|ARCHITECTURE|AGENTS|CLAUDE)\.md$'
# The JavaScript workspace and the brand assets the site copies; the Rust jobs read neither.
js_paths='^(apps|packages|brand)/|^(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|biome\.json|vitest\.config\.ts|\.node-version)$|^scripts/check-js-licenses\.mjs$'
# Everything the web job builds or depends on, including the Rust code compiled to wasm.
web_paths='^(apps|packages|brand)/|^crates/ariad-(core|wasm)/|^(package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|biome\.json|vitest\.config\.ts|\.node-version|Cargo\.toml|Cargo\.lock|rust-toolchain\.toml|justfile)$|^scripts/(ci-scope\.sh|install-wasm-bindgen\.sh)$|^\.github/workflows/ci\.yml$'

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
    if printf '%s\n' "$changed" | grep -v -E "$docs_paths|$js_paths" >/dev/null; then
        heavy=true
    fi
    web=false
    if printf '%s\n' "$changed" | grep -E "$web_paths" >/dev/null; then
        web=true
    fi
    if [ "$heavy" = true ]; then
        echo "$heavy|$web|code change"
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

if [ -n "${GITHUB_OUTPUT:-}" ]; then
    {
        printf 'range=%s\n' "$range"
        printf 'heavy=%s\n' "$heavy"
        printf 'web=%s\n' "$web"
        printf 'reason=%s\n' "$reason"
    } >> "$GITHUB_OUTPUT"
fi

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
        printf '### CI scope\n\n'
        printf -- '- Commits checked: `%s`\n' "${range:-none}"
        printf -- '- Build and test on three OSes: **%s** (%s)\n' "$heavy" "$reason"
        printf -- '- Web checks: **%s**\n' "$web"
    } >> "$GITHUB_STEP_SUMMARY"
fi

printf 'heavy=%s web=%s (%s), range=%s\n' "$heavy" "$web" "$reason" "${range:-none}"
