#!/usr/bin/env bash
set -euo pipefail

# Decide the release mode from the six Apple signing secrets in the environment.
#   all six set   -> prints "signed"
#   none set      -> prints "unsigned" (the unsigned DMG is a workflow artifact only, never a Release asset)
#   1-5 set       -> prints the missing names to stderr (and GITHUB_STEP_SUMMARY when set), exits 1
# Only presence is tested. Values are never printed.

REQUIRED_SECRETS=(
    APPLE_CERTIFICATE
    APPLE_CERTIFICATE_PASSWORD
    APPLE_SIGNING_IDENTITY
    APPLE_API_KEY_ID
    APPLE_API_ISSUER
    APPLE_API_KEY
)

present=0
missing=()
for name in "${REQUIRED_SECRETS[@]}"; do
    if [[ -n "${!name:-}" ]]; then
        present=$((present + 1))
    else
        missing+=("${name}")
    fi
done

if [[ "${present}" -eq "${#REQUIRED_SECRETS[@]}" ]]; then
    echo "signed"
elif [[ "${present}" -eq 0 ]]; then
    echo "unsigned"
else
    echo "::error::Partial Apple signing secrets: ${present} of ${#REQUIRED_SECRETS[@]} set. Missing: ${missing[*]}" >&2
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        {
            echo "### Error: partial Apple signing credentials"
            echo "${present} of ${#REQUIRED_SECRETS[@]} secrets are set in the release environment. Missing: ${missing[*]}."
        } >> "${GITHUB_STEP_SUMMARY}"
    fi
    exit 1
fi
