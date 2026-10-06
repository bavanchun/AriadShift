#!/bin/sh
# Decides, inside GitHub Actions, which commits to lint and whether the
# multi-OS build and test jobs must run. See docs/ci.md for the rules.
#
# Input (environment): EVENT_NAME, PR_BASE_SHA, PR_HEAD_SHA, PR_BASE_REF,
# PUSH_BEFORE, REF_NAME, GITHUB_REPOSITORY, GH_TOKEN.
# Output (GITHUB_OUTPUT): range, heavy, reason.
set -eu

zero='0000000000000000000000000000000000000000'
range=''
target_ref=''
changed=''

case "$EVENT_NAME" in
    pull_request)
        range="$PR_BASE_SHA..$PR_HEAD_SHA"
        changed=$(git diff --name-only "$PR_BASE_SHA...$PR_HEAD_SHA")
        target_ref=$PR_BASE_REF
        ;;
    push)
        if [ "$PUSH_BEFORE" != "$zero" ] && git cat-file -e "$PUSH_BEFORE^{commit}" 2>/dev/null; then
            range="$PUSH_BEFORE..HEAD"
            changed=$(git diff --name-only "$PUSH_BEFORE" HEAD)
        fi
        target_ref=$REF_NAME
        ;;
esac

decide() {
    # Manual runs and pushes without a known previous commit always run everything.
    if [ "$EVENT_NAME" = workflow_dispatch ] || [ -z "$changed" ]; then
        echo "true|full run"
        return
    fi

    # Prose-only changes: the static job already spell-checks them.
    if ! printf '%s\n' "$changed" | grep -v -E '^(docs|plans)/|^(README|ARCHITECTURE|AGENTS|CLAUDE)\.md$' >/dev/null; then
        echo "false|documentation-only change"
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
                    echo "false|tree already verified on dev at $sha"
                    return
                fi
            done
        else
            echo "true|could not list dev runs; running everything"
            return
        fi
    fi

    echo "true|code change"
}

decision=$(decide)
heavy=${decision%%|*}
reason=${decision#*|}

{
    printf 'range=%s\n' "$range"
    printf 'heavy=%s\n' "$heavy"
    printf 'reason=%s\n' "$reason"
} >> "$GITHUB_OUTPUT"

{
    printf '### CI scope\n\n'
    printf -- '- Commits checked: `%s`\n' "${range:-none}"
    printf -- '- Build and test on three OSes: **%s** (%s)\n' "$heavy" "$reason"
} >> "${GITHUB_STEP_SUMMARY:-/dev/null}"

printf 'heavy=%s (%s), range=%s\n' "$heavy" "$reason" "${range:-none}"
