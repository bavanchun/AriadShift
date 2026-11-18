#!/usr/bin/env bash
set -euo pipefail

# Decide the release mode from the six Apple signing secrets in the environment and a boolean that says whether the
# Sparkle EdDSA private key is set.
#
#   six Apple secrets  SPARKLE_ED_KEY_PRESENT  mode      what the workflow does
#   all six            true or false           signed    Developer ID, notarized (unchanged path)
#   none               true                    adhoc     ad-hoc signed, Sparkle-updated, published to the Release
#   none               false                   unsigned  workflow artifact only, never a Release asset
#   one to five        true or false           (fails)   fail closed: names the missing secrets and exits 1
#
# The mode is printed on stdout. A partial Apple set fails whatever the Sparkle flag says, so a half-configured
# Developer ID setup can never fall through to an ad-hoc release. Only presence is tested. Values are never printed.
#
# SPARKLE_ED_KEY_PRESENT is the workflow expression ${{ secrets.SPARKLE_ED_PRIVATE_KEY != '' }}, which renders as the
# string true or false. This step therefore never receives the private key, only whether it is set. Surrounding
# whitespace (spaces, tabs, newlines) is trimmed first, so a blank value counts as absent, the same as unset or empty.
# Any other value is a wiring error and fails closed rather than silently choosing a mode. The script never reads
# SPARKLE_ED_PRIVATE_KEY: a whitespace-only key cannot be seen from here (see the appcast step, which rejects it).

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

# Trim leading and trailing whitespace of the flag.
sparkle_flag="${SPARKLE_ED_KEY_PRESENT:-}"
sparkle_flag="${sparkle_flag#"${sparkle_flag%%[![:space:]]*}"}"
sparkle_flag="${sparkle_flag%"${sparkle_flag##*[![:space:]]}"}"
case "${sparkle_flag}" in
    true) sparkle_key_present=1 ;;
    '' | false) sparkle_key_present=0 ;;
    *)
        # The value is not echoed: if the key itself were wired here by mistake it must not reach the log.
        echo "::error::SPARKLE_ED_KEY_PRESENT must be true or false (the workflow passes secrets.SPARKLE_ED_PRIVATE_KEY != '')." >&2
        exit 1
        ;;
esac

if [[ "${present}" -eq "${#REQUIRED_SECRETS[@]}" ]]; then
    echo "signed"
elif [[ "${present}" -eq 0 ]]; then
    if [[ "${sparkle_key_present}" -eq 1 ]]; then
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
