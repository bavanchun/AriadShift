#!/usr/bin/env bash
set -euo pipefail

# Decide the release mode from the six Apple signing secrets and the Sparkle EdDSA private key in the environment.
#
#   six Apple secrets  SPARKLE_ED_PRIVATE_KEY  mode      what the workflow does
#   all six            set or unset            signed    Developer ID, notarized (unchanged path)
#   none               set                     adhoc     ad-hoc signed, Sparkle-updated, published to the Release
#   none               unset                   unsigned  workflow artifact only, never a Release asset
#   one to five        set or unset            (fails)   fail closed: names the missing secrets and exits 1
#
# The mode is printed on stdout. A partial Apple set fails whatever the Sparkle key says, so a half-configured
# Developer ID setup can never fall through to an ad-hoc release. Only presence is tested. Values are never printed.

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
    if [[ -n "${SPARKLE_ED_PRIVATE_KEY:-}" ]]; then
        echo "adhoc"
    else
        echo "unsigned"
    fi
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
