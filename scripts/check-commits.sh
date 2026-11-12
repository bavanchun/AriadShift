#!/bin/sh
# Checks every commit in a range against docs/git-workflow.md: Conventional
# Commits through committed, and no reference to an AI tool.
set -eu

range=${1:?usage: check-commits.sh <base>..<head>}
repo_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)

committed="$repo_root/.tools/bin/committed"
if [ ! -x "$committed" ] && [ -x "$committed.exe" ]; then
    committed="$committed.exe"
fi
if [ ! -x "$committed" ]; then
    printf '%s\n' 'committed is missing; run `just lint-tools` first.' >&2
    exit 1
fi

status=0
# Merge commits come from promotions and hotfix back-merges, and GitHub
# writes their subjects, so only the other commits must follow the format.
for sha in $(git rev-list --no-merges "$range"); do
    # Historical dev commits have overlong body lines; history must not be
    # rewritten. New commits remain fully checked.
    case "$sha" in
        8f9e88497cc79618e53ab27991a8cb75f215437c|60ea57f8a3bc772c3797e222e275f2d3c0d9a2f6|6e1357e0a153eeaa77220c3b4798381c10f899b6|f25b8254351efa60a578ecccc16d54fa353382d8|28adc2a2a923d486fb2c5715e9e0a776f36bfc9d|c767368be01ee026ec7aee10e8a44852f83226fe) continue ;;
    esac
    "$committed" --config "$repo_root/committed.toml" "$sha" || status=1
done

for sha in $(git rev-list "$range"); do
    message=$(git log -1 --format=%B "$sha")
    if printf '%s\n' "$message" | grep -i -w -E -q 'claude|anthropic|chatgpt|openai|codex|gemini|copilot' ||
        printf '%s\n' "$message" | grep -i -E -q 'ai[- ](generated|assisted)|🤖'; then
        printf '%s: commit message refers to an AI tool\n' "$(git rev-parse --short "$sha")" >&2
        status=1
    fi
done

exit "$status"
