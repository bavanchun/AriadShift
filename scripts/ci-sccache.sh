#!/usr/bin/env bash
# Start or stop the sccache compiler cache daemon with Cloudflare R2 (S3) backend.
# Falls back gracefully to direct compilation if credentials or sccache are missing,
# or if the daemon fails to start, guaranteeing no build failure from cache errors.
set -euo pipefail

action="${1:-}"
if [ -z "$action" ]; then
    echo "usage: $0 <start|stop>" >&2
    exit 1
fi

case "$action" in
    start)
        if ! command -v sccache >/dev/null 2>&1; then
            echo "sccache: binary not found in PATH; falling back to direct compilation"
            exit 0
        fi

        bucket="${SCCACHE_BUCKET:-${R2_CACHE_BUCKET:-}}"
        endpoint="${SCCACHE_ENDPOINT:-}"
        if [ -z "$endpoint" ] && [ -n "${R2_ACCOUNT_ID:-}" ]; then
            endpoint="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"
        fi
        key_id="${AWS_ACCESS_KEY_ID:-${R2_ACCESS_KEY_ID:-}}"
        secret_key="${AWS_SECRET_ACCESS_KEY:-${R2_SECRET_ACCESS_KEY:-}}"

        if [ -z "$bucket" ] || [ -z "$endpoint" ] || [ -z "$key_id" ] || [ -z "$secret_key" ]; then
            echo "sccache: R2 credentials or configuration missing (e.g. fork PR); falling back to direct compilation"
            exit 0
        fi

        export SCCACHE_BUCKET="$bucket"
        export SCCACHE_ENDPOINT="$endpoint"
        export SCCACHE_REGION="${SCCACHE_REGION:-auto}"
        export SCCACHE_S3_KEY_PREFIX="${SCCACHE_S3_KEY_PREFIX:-ariadshift-sccache/}"
        export SCCACHE_S3_RW_MODE="${SCCACHE_S3_RW_MODE:-READ_ONLY}"
        export SCCACHE_IDLE_TIMEOUT=0
        export AWS_ACCESS_KEY_ID="$key_id"
        export AWS_SECRET_ACCESS_KEY="$secret_key"

        if sccache --start-server; then
            sccache --zero-stats >/dev/null 2>&1 || true
            if [ -n "${GITHUB_ENV:-}" ] && [ -w "$GITHUB_ENV" ]; then
                {
                    printf 'RUSTC_WRAPPER=sccache\n'
                    printf 'SCCACHE_ACTIVE=1\n'
                    printf 'SCCACHE_BUCKET=%s\n' "$SCCACHE_BUCKET"
                    printf 'SCCACHE_ENDPOINT=%s\n' "$SCCACHE_ENDPOINT"
                    printf 'SCCACHE_REGION=%s\n' "$SCCACHE_REGION"
                    printf 'SCCACHE_S3_KEY_PREFIX=%s\n' "$SCCACHE_S3_KEY_PREFIX"
                    printf 'SCCACHE_S3_RW_MODE=%s\n' "$SCCACHE_S3_RW_MODE"
                } >> "$GITHUB_ENV"
            fi
            echo "sccache: daemon started successfully (mode: $SCCACHE_S3_RW_MODE, prefix: $SCCACHE_S3_KEY_PREFIX)"
        else
            echo "::warning::sccache: failed to start daemon; falling back to direct compilation"
        fi
        ;;

    stop)
        if ! command -v sccache >/dev/null 2>&1; then
            exit 0
        fi

        job_name="${JOB_NAME:-Rust}"
        if sccache --show-stats >/dev/null 2>&1; then
            stats="$(sccache --show-stats 2>&1)"
            printf '=== sccache statistics (%s) ===\n%s\n' "$job_name" "$stats"
            if [ -n "${GITHUB_STEP_SUMMARY:-}" ] && [ -w "$GITHUB_STEP_SUMMARY" ]; then
                cat <<EOF >> "$GITHUB_STEP_SUMMARY"
### sccache Statistics ($job_name)

- **Mode:** \`${SCCACHE_S3_RW_MODE:-unknown}\`
- **Prefix:** \`${SCCACHE_S3_KEY_PREFIX:-ariadshift-sccache/}\`

\`\`\`text
$stats
\`\`\`

EOF
            fi
            sccache --stop-server >/dev/null 2>&1 || true
        else
            echo "sccache: daemon was not active for $job_name"
        fi
        ;;

    *)
        echo "unknown action: $action (expected 'start' or 'stop')" >&2
        exit 1
        ;;
esac
