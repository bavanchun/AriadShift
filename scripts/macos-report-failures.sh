#!/usr/bin/env bash
# Prints the failed tests and the build or test issues recorded in an xcodebuild result bundle.
# xcodebuild can end with "TEST FAILED" and an empty failing-tests list (for example after the test host lost its
# connection to a system service); the bundle still records which test or issue caused it.
set -uo pipefail

# Same location as the macos-test recipe: MACOS_RESULT_BUNDLE, else the default under target/derived-data.
bundle="${1:-${MACOS_RESULT_BUNDLE:-target/derived-data/AriadShift-tests.xcresult}}"
if [[ ! -d "$bundle" ]]; then
    echo "No result bundle at ${bundle}; the job failed before xcodebuild test ran."
    exit 0
fi

# Each query is best effort: this runs after a failure and must never hide it behind a second one.
query() {
    local title="$1"
    shift
    echo "::group::${title}"
    xcrun xcresulttool get "$@" --path "$bundle" --compact 2>&1 || echo "(${title} unavailable)"
    echo
    echo "::endgroup::"
}

echo "::group::Failed tests"
xcrun xcresulttool get test-results tests --path "$bundle" --compact 2>/dev/null |
    jq -r '
        [.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed")]
        | if length == 0 then "No failed test case recorded in the bundle."
          else .[] | "FAILED \(.nodeIdentifierURL // .name)" end' 2>&1 ||
    echo "(failed-test listing unavailable)"
echo "::endgroup::"

query "Test summary, failures and insights" test-results summary
query "Build results and issues" build-results
exit 0
