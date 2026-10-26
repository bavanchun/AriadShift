#!/usr/bin/env bash
# End-to-end test of the self-hosted cloud stack in infra/compose.
#
#   infra/test/e2e/run.sh
#
# Builds and starts the compose stack with the core, office and docling
# profiles under a throwaway environment generated from .env.example, then
# drives it as a client would: sign up through the auth service, create an
# API key, upload a scanned PDF through /v1, convert it to DOCX on the
# docling worker, follow the job's events, check the DOCX, delete the job,
# and confirm that the plan's upload allowance is enforced. The stack and its
# volumes are always removed on exit; on failure the compose logs are printed
# first.
#
# Needs docker (with the compose plugin and BuildKit), curl, jq and unzip.
# The host must meet docs/self-hosting.md's prerequisites for the office and
# docling profiles (Landlock, seccomp, unprivileged user namespaces, memory).
#
# Optional environment:
#   E2E_PROJECT          compose project name (default ariadshift-e2e)
#   E2E_API_PORT, E2E_AUTH_PORT, E2E_S3_PORT, E2E_WEB_PORT
#                        host ports (defaults 18080, 13001, 18333, 13000), kept
#                        apart from a developer stack on the default ports
#   E2E_WAIT_TIMEOUT     seconds to wait for healthy services (default 900)
#   E2E_JOB_TIMEOUT      seconds to wait for the conversion (default 900)
#   E2E_LOG_TAIL         log lines per service printed on failure (default 400)

set -eu
set -o pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
COMPOSE_FILE="$ROOT/infra/compose/docker-compose.yml"
ENV_EXAMPLE="$ROOT/.env.example"

# Scanned page with a recorded source and licence in fixtures/manifest.toml
# (id pd-en-alice-page-3). The DOCX must contain these words.
FIXTURE="$ROOT/fixtures/scan/en-alice-p3.pdf"
FIXTURE_SHA256=a94c88386277168d3ae1fc24cff44130b84ac916bd4cfd2cd29b4807b762f406
EXPECTED_WORDS="Alice rabbit"

PROJECT=${E2E_PROJECT:-ariadshift-e2e}
API_PORT=${E2E_API_PORT:-18080}
AUTH_PORT=${E2E_AUTH_PORT:-13001}
S3_PORT=${E2E_S3_PORT:-18333}
WEB_PORT=${E2E_WEB_PORT:-13000}
WAIT_TIMEOUT=${E2E_WAIT_TIMEOUT:-900}
JOB_TIMEOUT=${E2E_JOB_TIMEOUT:-900}
LOG_TAIL=${E2E_LOG_TAIL:-400}

API="http://localhost:$API_PORT"
AUTH="http://localhost:$AUTH_PORT"
WEB_ORIGIN="http://localhost:$WEB_PORT"

log() { printf 'e2e: %s\n' "$*"; }
fail() {
	printf 'e2e: FAIL: %s\n' "$*" >&2
	exit 1
}

for tool in docker curl jq unzip; do
	command -v "$tool" >/dev/null 2>&1 || fail "$tool is required"
done
docker compose version >/dev/null 2>&1 || fail "the docker compose plugin is required"
[ -f "$FIXTURE" ] || fail "missing fixture $FIXTURE"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ariadshift-e2e.XXXXXX")
ENV_FILE="$WORK/env"
BODY="$WORK/body"
STACK_STARTED=0

compose() {
	docker compose -p "$PROJECT" --env-file "$ENV_FILE" -f "$COMPOSE_FILE" \
		--profile core --profile office --profile docling "$@"
}

cleanup() {
	rc=$?
	trap - EXIT INT TERM
	if [ "$STACK_STARTED" = 1 ]; then
		if [ "$rc" -ne 0 ]; then
			log "stack state and logs follow"
			compose ps -a || true
			compose logs --no-color --timestamps --tail "$LOG_TAIL" || true
		fi
		log "tearing down project $PROJECT"
		compose down --volumes --remove-orphans --timeout 30 >/dev/null 2>&1 || true
	fi
	rm -rf "$WORK"
	if [ "$rc" -eq 0 ]; then log "PASS"; fi
	exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

random_hex() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d' ' -f1
	else
		shasum -a 256 "$1" | cut -d' ' -f1
	fi
}

size_of() { wc -c <"$1" | tr -d ' '; }

# request METHOD URL [curl args...]: stores the body in $BODY and prints the
# HTTP status. Transport errors print 000.
request() {
	method=$1 url=$2
	shift 2
	curl -sS -o "$BODY" -w '%{http_code}' -X "$method" "$@" "$url" || true
}

# expect WANT GOT WHAT: fails with the response body unless the status matches.
expect() {
	if [ "$2" != "$1" ]; then
		printf 'e2e: response body: %s\n' "$(head -c 4096 "$BODY" 2>/dev/null)" >&2
		fail "$3: expected HTTP $1, got $2"
	fi
}

# field JQ_FILTER: one value from the last response body; fails when empty.
field() {
	value=$(jq -er "$1 // empty" "$BODY") || fail "response has no $1: $(head -c 4096 "$BODY")"
	printf '%s' "$value"
}

v1() {
	method=$1 path=$2
	shift 2
	request "$method" "$API$path" -H "x-api-key: $API_KEY" "$@"
}

# --- Throwaway environment -------------------------------------------------

log "writing a throwaway environment"
while IFS= read -r line || [ -n "$line" ]; do
	case $line in
	*=replace-with-*) printf '%s=%s\n' "${line%%=*}" "$(random_hex)" ;;
	COMPOSE_PROFILES=*) printf 'COMPOSE_PROFILES=core,office,docling\n' ;;
	ARIAD_PACKS=*) printf 'ARIAD_PACKS=core,office,docling\n' ;;
	BIND_ADDRESS=*) printf 'BIND_ADDRESS=127.0.0.1\n' ;;
	WEB_PORT=*) printf 'WEB_PORT=%s\n' "$WEB_PORT" ;;
	API_PORT=*) printf 'API_PORT=%s\n' "$API_PORT" ;;
	AUTH_PORT=*) printf 'AUTH_PORT=%s\n' "$AUTH_PORT" ;;
	S3_PORT=*) printf 'S3_PORT=%s\n' "$S3_PORT" ;;
	ARIAD_WEB_ORIGIN=*) printf 'ARIAD_WEB_ORIGIN=%s\n' "$WEB_ORIGIN" ;;
	ARIAD_PUBLIC_URL=*) printf 'ARIAD_PUBLIC_URL=%s\n' "$API" ;;
	BETTER_AUTH_URL=*) printf 'BETTER_AUTH_URL=%s\n' "$AUTH" ;;
	ARIAD_S3_PUBLIC_ENDPOINT=*) printf 'ARIAD_S3_PUBLIC_ENDPOINT=http://localhost:%s\n' "$S3_PORT" ;;
	ARIAD_ANONYMOUS=*) printf 'ARIAD_ANONYMOUS=true\n' ;;
	ARIAD_TURNSTILE_SECRET=*) printf 'ARIAD_TURNSTILE_SECRET=\n' ;;
	*) printf '%s\n' "$line" ;;
	esac
done <"$ENV_EXAMPLE" >"$ENV_FILE"
if grep -q '^[A-Z0-9_]*=replace-with' "$ENV_FILE"; then
	fail "the generated environment still has placeholder values"
fi

# --- Stack -----------------------------------------------------------------

STACK_STARTED=1
log "building images"
compose build
log "starting the stack (waiting up to ${WAIT_TIMEOUT}s for health)"
compose up -d --wait --wait-timeout "$WAIT_TIMEOUT"

# Health checks run inside the containers; confirm the published ports too.
wait_ok() {
	deadline=$(($(date +%s) + 120))
	until [ "$(curl -s -o /dev/null -w '%{http_code}' "$1" || true)" = 200 ]; do
		[ "$(date +%s)" -lt "$deadline" ] || fail "$1 did not become ready"
		sleep 2
	done
}
wait_ok "$API/readyz"
wait_ok "$AUTH/api/auth/ok"
log "stack is healthy"

# Preserve the disposable database before the client journey changes its data.
compose exec -T postgres pg_dump -U postgres -d ariadshift -Fc >"$WORK/before-journey.dump"

# --- Account and API key ---------------------------------------------------

JAR="$WORK/cookies"
EMAIL="e2e-$(random_hex | cut -c1-16)@example.com"
PASSWORD=$(random_hex)

log "signing up $EMAIL"
code=$(request POST "$AUTH/api/auth/sign-up/email" -c "$JAR" -b "$JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data "$(jq -nc --arg e "$EMAIL" --arg p "$PASSWORD" '{name: "E2E", email: $e, password: $p}')")
expect 200 "$code" "sign-up"
field '.user.id' >/dev/null

log "creating an API key"
code=$(request POST "$AUTH/api/auth/api-key/create" -c "$JAR" -b "$JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data '{"name":"e2e"}')
expect 200 "$code" "API key creation"
API_KEY=$(field '.key')
case $API_KEY in ashift_*) ;; *) fail "the API key lacks the ashift_ prefix" ;; esac

# --- Upload ----------------------------------------------------------------

[ "$(sha256_of "$FIXTURE")" = "$FIXTURE_SHA256" ] || fail "fixture hash differs from the manifest"
SIZE=$(size_of "$FIXTURE")

log "creating an upload ($SIZE bytes)"
code=$(v1 POST /v1/uploads -H 'content-type: application/json' \
	--data "$(jq -nc --argjson s "$SIZE" --arg h "$FIXTURE_SHA256" \
		'{filename: "alice-scan.pdf", size: $s, sha256: $h}')")
expect 201 "$code" "upload creation"
UPLOAD_ID=$(field '.id')
[ "$(field '.status')" = pending ] || fail "a new upload is not pending"
[ "$(field '.upload.method')" = PUT ] || fail "the presigned request is not a PUT"
PUT_URL=$(field '.upload.url')
put_headers=()
while IFS= read -r header; do
	put_headers+=(-H "$header")
done < <(jq -r '.upload.headers // {} | to_entries[] | "\(.key): \(.value)"' "$BODY")

log "uploading the scanned PDF to object storage"
# --upload-file sends no Content-Type of its own; only the signed headers go.
code=$(request PUT "$PUT_URL" --upload-file "$FIXTURE" ${put_headers[@]+"${put_headers[@]}"})
expect 200 "$code" "presigned PUT"

code=$(v1 POST "/v1/uploads/$UPLOAD_ID/complete")
expect 200 "$code" "upload completion"
[ "$(field '.status')" = completed ] || fail "the upload did not complete"
[ "$(field '.format')" = pdf ] || fail "the upload was not detected as PDF"

# --- Conversion ------------------------------------------------------------

log "creating a PDF to DOCX job"
code=$(v1 POST /v1/jobs -H 'content-type: application/json' \
	--data "$(jq -nc --arg u "$UPLOAD_ID" '{upload_id: $u, to: "docx"}')")
expect 202 "$code" "job creation"
JOB_ID=$(field '.id')
[ "$(field '.pack')" = docling ] || fail "the job was not routed to the docling pack"
[ "$(field '.route[0].engine')" = docling ] || fail "the route does not start with docling"

# Follows the event stream, resuming with Last-Event-ID whenever the stream
# closes without a final event (the stream itself ends after one). Between
# streams the job is polled, so a broken stream cannot hide a finished job.
log "following job $JOB_ID"
EVENTS="$WORK/events"
last_id=""
final=""
deadline=$(($(date +%s) + JOB_TIMEOUT))
while [ -z "$final" ]; do
	[ "$(date +%s)" -lt "$deadline" ] || fail "the job did not finish within ${JOB_TIMEOUT}s"
	resume=()
	[ -z "$last_id" ] || resume=(-H "last-event-id: $last_id")
	: >"$EVENTS"
	curl -sSN --max-time 120 -o "$EVENTS" -H 'accept: text/event-stream' \
		-H "x-api-key: $API_KEY" ${resume[@]+"${resume[@]}"} \
		"$API/v1/jobs/$JOB_ID/events" || true
	while IFS= read -r line; do
		case $line in
		data:*)
			event=${line#data:}
			kind=$(printf '%s' "$event" | jq -r '.kind // empty' 2>/dev/null || true)
			id=$(printf '%s' "$event" | jq -r '.id // empty' 2>/dev/null || true)
			[ -n "$id" ] && last_id=$id
			[ -n "$kind" ] && log "event: $kind"
			case $kind in succeeded | failed | canceled | expired) final=$kind ;; esac
			;;
		esac
	done <"$EVENTS"
	[ -z "$final" ] || break
	code=$(v1 GET "/v1/jobs/$JOB_ID")
	expect 200 "$code" "job poll"
	status=$(field '.status')
	log "polled status: $status"
	case $status in
	succeeded | failed | canceled | expired) final=$status ;;
	*) sleep 2 ;;
	esac
done

code=$(v1 GET "/v1/jobs/$JOB_ID")
expect 200 "$code" "job lookup"
status=$(field '.status')
if [ "$final" != succeeded ] || [ "$status" != succeeded ]; then
	fail "job ended with event '$final' and status '$status': $(jq -c '.error' "$BODY")"
fi

# --- Output ----------------------------------------------------------------

code=$(v1 GET "/v1/jobs/$JOB_ID/outputs")
expect 200 "$code" "output listing"
OUTPUT=$(jq -ec '[.outputs[] | select(.format == "docx")][0]' "$BODY") || fail "no DOCX output"
OUT_URL=$(printf '%s' "$OUTPUT" | jq -r '.url')
OUT_SHA=$(printf '%s' "$OUTPUT" | jq -r '.sha256')
OUT_SIZE=$(printf '%s' "$OUTPUT" | jq -r '.size')

DOCX="$WORK/output.docx"
log "downloading the DOCX ($OUT_SIZE bytes)"
code=$(curl -sS -o "$DOCX" -w '%{http_code}' "$OUT_URL" || true)
[ "$code" = 200 ] || fail "DOCX download returned HTTP $code"
[ "$(size_of "$DOCX")" = "$OUT_SIZE" ] || fail "the DOCX size differs from the listing"
[ "$(sha256_of "$DOCX")" = "$OUT_SHA" ] || fail "the DOCX hash differs from the listing"

unzip -tqq "$DOCX" >/dev/null || fail "the DOCX is not a valid ZIP archive"
entries=$(unzip -Z1 "$DOCX")
for entry in '[Content_Types].xml' word/document.xml; do
	grep -Fqx "$entry" <<<"$entries" || fail "the DOCX lacks $entry"
done
# Strip the markup so words split across runs still match.
text=$(unzip -p "$DOCX" word/document.xml | sed -e 's/<[^>]*>//g')
for word in $EXPECTED_WORDS; do
	grep -Fqi "$word" <<<"$text" || fail "the DOCX text lacks '$word'"
done
log "the DOCX is valid and contains: $EXPECTED_WORDS"

# --- Deletion --------------------------------------------------------------

log "deleting the job"
code=$(v1 DELETE "/v1/jobs/$JOB_ID")
expect 204 "$code" "job deletion"
code=$(v1 GET "/v1/jobs/$JOB_ID")
expect 200 "$code" "deleted job lookup"
[ "$(field '.status')" = expired ] || fail "a deleted job is not expired"
code=$(v1 GET "/v1/jobs/$JOB_ID/outputs")
expect 410 "$code" "outputs of a deleted job"
code=$(curl -s -o /dev/null -w '%{http_code}' "$OUT_URL" || true)
[ "$code" != 200 ] || fail "the deleted DOCX is still downloadable"

# --- Quota refusal ---------------------------------------------------------

# Pending uploads reserve the plan's concurrent-job slots, so one more than
# the cap must be refused with quota_exceeded.
CAP=$(compose exec -T postgres psql -X -qtA -U postgres -d ariadshift \
	-c "SELECT concurrent_jobs FROM plan_entitlements WHERE plan = 'free'" | tr -d '[:space:]')
case $CAP in '' | *[!0-9]*) fail "cannot read the free plan's concurrent_jobs: '$CAP'" ;; esac
log "checking the upload allowance (free plan reserves $CAP pending uploads)"
i=1
while [ "$i" -le "$CAP" ]; do
	code=$(v1 POST /v1/uploads -H 'content-type: application/json' \
		--data "$(jq -nc --arg n "pending-$i.md" --arg h "$(random_hex)" \
			'{filename: $n, size: 16, sha256: $h}')")
	expect 201 "$code" "pending upload $i of $CAP"
	i=$((i + 1))
done
code=$(v1 POST /v1/uploads -H 'content-type: application/json' \
	--data "$(jq -nc --arg h "$(random_hex)" '{filename: "one-too-many.md", size: 16, sha256: $h}')")
expect 429 "$code" "upload beyond the allowance"
[ "$(field '.error.code')" = quota_exceeded ] || fail "the refusal is not quota_exceeded"
log "upload $((CAP + 1)) was refused with quota_exceeded"

# --- Account deletion and credential isolation -----------------------------

log "checking account deletion revokes only the deleted user's API keys"
code=$(request POST "$AUTH/api/auth/api-key/create" -c "$JAR" -b "$JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data '{"name":"deletion-check"}')
expect 200 "$code" "second API key creation"
EXTRA_KEY=$(field '.key')

OTHER_JAR="$WORK/other-cookies"
code=$(request POST "$AUTH/api/auth/sign-up/email" -c "$OTHER_JAR" -b "$OTHER_JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data "$(jq -nc --arg e "other-$EMAIL" --arg p "$(random_hex)" \
		'{name: "Other E2E", email: $e, password: $p}')")
expect 200 "$code" "other account sign-up"
code=$(request POST "$AUTH/api/auth/api-key/create" -c "$OTHER_JAR" -b "$OTHER_JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data '{"name":"other-owner"}')
expect 200 "$code" "other account API key creation"
OTHER_KEY=$(field '.key')

check_key() {
	key=$1 wanted=$2 description=$3
	code=$(request POST "$API/v1/plan" -H "x-api-key: $key" \
		-H 'content-type: application/json' --data '{"from":"markdown","to":"html"}')
	expect "$wanted" "$code" "$description"
}
check_key "$API_KEY" 200 "original key before account deletion"
check_key "$EXTRA_KEY" 200 "second key before account deletion"
check_key "$OTHER_KEY" 200 "other owner's key before account deletion"

compose exec -T postgres pg_dump -U postgres -d ariadshift -Fc >"$WORK/before-deletion.dump"
code=$(request POST "$AUTH/api/auth/delete-user" -c "$JAR" -b "$JAR" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data "$(jq -nc --arg p "$PASSWORD" '{password: $p}')")
expect 200 "$code" "password-confirmed account deletion"
check_key "$API_KEY" 401 "original key after account deletion"
check_key "$EXTRA_KEY" 401 "second key after account deletion"
check_key "$OTHER_KEY" 200 "other owner's key after account deletion"
log "deleted account keys are rejected; the other owner's key still works"
