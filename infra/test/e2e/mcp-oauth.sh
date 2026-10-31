#!/usr/bin/env bash
# End-to-end test of interactive OAuth for remote MCP on the compose stack.
#
#   infra/test/e2e/mcp-oauth.sh        (or: just mcp-oauth-e2e)
#
# Starts the core profile of infra/compose under a throwaway environment
# generated from .env.example, then acts as an MCP client would:
#
#   1. POST /mcp without credentials and follow the RFC 9728 challenge to
#      the protected-resource metadata and the authorization server;
#   2. read the RFC 8414 metadata and register a public client (RFC 7591);
#   3. check that authorization without PKCE, or with the plain method, is
#      refused;
#   4. authorize with S256 PKCE and the MCP resource (RFC 8707), sign in on
#      the sign-in page's endpoint and approve on the consent page's endpoint;
#   5. exchange the code, check the access token's claims, and call MCP
#      `server/discover` and `tools/list` over Streamable HTTP with it;
#   6. upload Markdown, convert via MCP, poll job_status and download HTML;
#   7. refresh the token and use the new one;
#   8. confirm that the ordinary API JWT (audience: the auth origin) is
#      refused at /mcp with 401 and a resource-metadata challenge.
#
# MCP 2026-07-28 is stateless: `server/discover` replaces the old
# `initialize` handshake. Tokens are never printed. The stack and its volumes
# are always removed on exit; on failure only the stack state is printed.
#
# Needs docker (with the compose plugin and BuildKit), curl, jq and openssl.
#
# Optional environment:
#   E2E_PROJECT          compose project name (default ariadshift-e2e-mcp)
#   E2E_API_PORT, E2E_AUTH_PORT, E2E_S3_PORT, E2E_WEB_PORT
#                        host ports (defaults 18180, 13101, 18433, 13100), kept
#                        apart from a developer stack and from run.sh
#   E2E_WAIT_TIMEOUT     seconds to wait for healthy services (default 600)

set -eu
umask 077
set -o pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
COMPOSE_FILE="$ROOT/infra/compose/docker-compose.yml"
ENV_EXAMPLE="$ROOT/.env.example"

PROJECT=${E2E_PROJECT:-ariadshift-e2e-mcp}
API_PORT=${E2E_API_PORT:-18180}
AUTH_PORT=${E2E_AUTH_PORT:-13101}
S3_PORT=${E2E_S3_PORT:-18433}
WEB_PORT=${E2E_WEB_PORT:-13100}
WAIT_TIMEOUT=${E2E_WAIT_TIMEOUT:-600}

API="http://localhost:$API_PORT"
AUTH="http://localhost:$AUTH_PORT"
WEB_ORIGIN="http://localhost:$WEB_PORT"
RESOURCE="$API/mcp"
REDIRECT_URI="http://127.0.0.1:33418/callback"
MCP_VERSION="2026-07-28"

log() { printf 'e2e-mcp: %s\n' "$*"; }
fail() {
	printf 'e2e-mcp: FAIL: %s\n' "$*" >&2
	exit 1
}

for tool in docker curl jq openssl; do
	command -v "$tool" >/dev/null 2>&1 || fail "$tool is required"
done
docker compose version >/dev/null 2>&1 || fail "the docker compose plugin is required"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/ariadshift-e2e-mcp.XXXXXX")
ENV_FILE="$WORK/env"
BODY="$WORK/body"
HEADERS="$WORK/headers"
JAR="$WORK/cookies"
STACK_STARTED=0

compose() {
	docker compose -p "$PROJECT" --env-file "$ENV_FILE" -f "$COMPOSE_FILE" -f "$WORK/ownership.yml" --profile core "$@"
}

# Compose labels bind cleanup to this worktree, including volumes left by a kill.
assert_owned_project() {
	case "$PROJECT" in ariadshift-*) ;; *) fail "use a dedicated ariadshift-* test project" ;; esac
	local object owner mounted_volumes=""
	for object in $(docker ps -aq --filter "label=com.docker.compose.project=$PROJECT"); do
		owner=$(docker inspect -f '{{ index .Config.Labels "com.docker.compose.project.working_dir" }}' "$object")
		[ "$owner" = "$(dirname "$COMPOSE_FILE")" ] || fail "project $PROJECT belongs to another worktree"
		mounted_volumes="$mounted_volumes
$(docker inspect -f '{{ range .Mounts }}{{ if eq .Type "volume" }}{{ .Name }}{{ "\n" }}{{ end }}{{ end }}' "$object")"
	done
	for object in $(docker volume ls -q --filter "label=com.docker.compose.project=$PROJECT"); do
		owner=$(docker volume inspect -f '{{ index .Labels "ariadshift.test-worktree" }}' "$object")
		if [ "$owner" != "$ROOT" ]; then
			case "$owner" in
				'' | '<no value>') ;;
				*) fail "volume $object belongs to another worktree; preserve it" ;;
			esac
			# Older runs lack the custom label. Require a mount from a container
			# whose Compose working directory was verified above.
			printf '%s\n' "$mounted_volumes" | grep -Fxq "$object" ||
				fail "volume $object has no matching test owner; preserve it"
		fi
	done
}

reset_project() {
	assert_owned_project
	compose down --volumes --remove-orphans --timeout 30 >/dev/null
}

cleanup() {
	rc=$?
	trap - EXIT INT TERM
	if [ "$STACK_STARTED" = 1 ]; then
		# Arbitrary service logs can contain credentials; retain only stack state.
		if [ "$rc" -ne 0 ]; then compose ps -a || true; fi
		log "tearing down project $PROJECT"
		(reset_project) || rc=1
	fi
	rm -rf "$WORK"
	if [ "$rc" -eq 0 ]; then log "PASS"; fi
	exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Private inputs are files under the mode-700 temporary directory.
private_file() {
	printf '%s' "$2" >"$WORK/$1"
	printf '%s' "$WORK/$1"
}

random_hex() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
base64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

# request METHOD URL [curl args...]: stores the body in $BODY and the
# response headers in $HEADERS, and prints the HTTP status (000 on a
# transport error). Redirects are never followed.
request() {
	method=$1 url=$2
	shift 2
	# URLs can carry authorization codes or signed query strings too.
	printf '%s' "$url" | jq -Rrs '"url = " + tojson' >"$WORK/request.conf"
	curl -sS --config "$WORK/request.conf" -o "$BODY" -D "$HEADERS" -w '%{http_code}' -X "$method" "$@" || true
}

# Never print raw or malformed bodies: these endpoints can return credentials.
diagnostic_body() {
	# Slurp validates the entire input before filtering or printing a prefix.
	# Emit only known protocol error names and a numeric status. Descriptions,
	# unknown keys, nested values and arbitrary strings can contain credentials.
	local diagnostic
	if diagnostic=$(jq -cse '
	  if length == 1 and (.[0] | type == "object") then .[0] |
	    {error: (.error | if . == "invalid_request" or . == "invalid_grant"
	      or . == "invalid_client" or . == "invalid_target" or . == "invalid_scope"
	      or . == "unauthorized_client" or . == "unsupported_grant_type"
	      or . == "access_denied" or . == "server_error"
	      or . == "temporarily_unavailable" then . else null end),
	     status: (.status | if type == "number" then . else null end)} |
	    with_entries(select(.value != null))
	  else error("unsupported response") end' "$BODY" 2>/dev/null); then
	  printf '%s' "$diagnostic" | head -c 2048
	else
	  printf '[response omitted]'
	fi
}

# expect WANT GOT WHAT: fails with the response body unless the status matches.
expect() {
	if [ "$2" != "$1" ]; then
		printf 'e2e-mcp: response body: %s\n' "$(diagnostic_body)" >&2
		fail "$3: expected HTTP $1, got $2"
	fi
}

# field JQ_FILTER: one value from the last response body; fails when empty.
field() {
	value=$(jq -er "$1 // empty" "$BODY") || fail "response has no $1: $(diagnostic_body)"
	printf '%s' "$value"
}

# header NAME: one header of the last response, without the trailing CR.
header() {
	grep -i "^$1:" "$HEADERS" | head -n 1 | cut -d: -f2- | sed 's/^ *//' | tr -d '\r'
}

# param URL NAME: one query parameter of URL, still percent-encoded.
param() {
	printf '%s' "$1" | jq -Rr --arg n "$2" 'capture("[?&]" + $n + "=(?<v>[^&#]*)").v // empty'
}

# uri VALUE: VALUE percent-encoded the way URLSearchParams encodes it.
uri() { jq -rn --arg v "$1" '$v | @uri'; }

# The whole query of a page the provider redirected to. Better Auth signs
# every parameter it adds there, so this is the `oauth_query` its pages send.
query_of() { printf '%s' "${1#*\?}"; }

# claims TOKEN: the decoded JWT payload, for jq.
claims() {
	payload=$(printf '%s' "$1" | cut -d. -f2 | tr '_-' '/+')
	while [ $((${#payload} % 4)) -ne 0 ]; do payload="$payload="; done
	printf '%s' "$payload" | openssl base64 -d -A
}

# mcp TOKEN METHOD: one stateless MCP request with a Bearer token.
mcp() {
	params=${3-'{}'}
	request POST "$RESOURCE" \
		-H @"$(private_file bearer "authorization: Bearer $1")" \
		-H 'content-type: application/json' \
		-H 'accept: application/json, text/event-stream' \
		-H "mcp-protocol-version: $MCP_VERSION" \
		-H "mcp-method: $2" \
		-H "mcp-name: $(printf '%s' "$params" | jq -r '.name // empty')" \
		--data "$(jq -nc --arg m "$2" --arg v "$MCP_VERSION" --argjson p "$params" '{
			jsonrpc: "2.0", id: 1, method: $m,
			params: ($p + {_meta: {
				"io.modelcontextprotocol/protocolVersion": $v,
				"io.modelcontextprotocol/clientInfo": {name: "ariadshift-e2e", version: "1"},
				"io.modelcontextprotocol/clientCapabilities": {}
			}})
		}')"
}

# --- Throwaway environment -------------------------------------------------

log "writing a throwaway environment"
while IFS= read -r line || [ -n "$line" ]; do
	case $line in
	*=replace-with-*) printf '%s=%s\n' "${line%%=*}" "$(random_hex)" ;;
	COMPOSE_PROFILES=*) printf 'COMPOSE_PROFILES=core\n' ;;
	ARIAD_PACKS=*) printf 'ARIAD_PACKS=core\n' ;;
	BIND_ADDRESS=*) printf 'BIND_ADDRESS=127.0.0.1\n' ;;
	WEB_PORT=*) printf 'WEB_PORT=%s\n' "$WEB_PORT" ;;
	API_PORT=*) printf 'API_PORT=%s\n' "$API_PORT" ;;
	AUTH_PORT=*) printf 'AUTH_PORT=%s\n' "$AUTH_PORT" ;;
	S3_PORT=*) printf 'S3_PORT=%s\n' "$S3_PORT" ;;
	ARIAD_WEB_ORIGIN=*) printf 'ARIAD_WEB_ORIGIN=%s\n' "$WEB_ORIGIN" ;;
	ARIAD_PUBLIC_URL=*) printf 'ARIAD_PUBLIC_URL=%s\n' "$API" ;;
	BETTER_AUTH_URL=*) printf 'BETTER_AUTH_URL=%s\n' "$AUTH" ;;
	ARIAD_S3_PUBLIC_ENDPOINT=*) printf 'ARIAD_S3_PUBLIC_ENDPOINT=http://localhost:%s\n' "$S3_PORT" ;;
	ARIAD_TURNSTILE_SECRET=*) printf 'ARIAD_TURNSTILE_SECRET=\n' ;;
	*) printf '%s\n' "$line" ;;
	esac
done <"$ENV_EXAMPLE" >"$ENV_FILE"
# Explicit test-only signup opt-in, including when an older template lacks it.
printf '\nAUTH_ALLOW_SIGN_UP=true\n' >>"$ENV_FILE"
# Labels persist after interrupted runs; unknown or foreign volumes are preserved.
printf 'volumes:\n  postgres-data:\n    labels:\n      ariadshift.test-worktree: %s\n  seaweedfs-data:\n    labels:\n      ariadshift.test-worktree: %s\n' "$ROOT" "$ROOT" >"$WORK/ownership.yml"
if grep -q '^[A-Z0-9_]*=replace-with' "$ENV_FILE"; then
	fail "the generated environment still has placeholder values"
fi

# --- Stack -----------------------------------------------------------------

reset_project
STACK_STARTED=1
# Start the conversion worker too; shared dependencies start with these services.
log "building images"
for service in migrate auth; do
	COMPOSE_PARALLEL_LIMIT=1 compose build --build-arg CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" "$service"
done
log "starting the auth service and the API (waiting up to ${WAIT_TIMEOUT}s for health)"
compose up -d --wait --wait-timeout "$WAIT_TIMEOUT" auth api worker-core

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

EMAIL="e2e-$(random_hex | cut -c1-16)@example.com"
PASSWORD=$(random_hex)
export PASSWORD
log "creating the account $EMAIL"
jq -nc --arg e "$EMAIL" '{name: "E2E", email: $e, password: $ENV.PASSWORD}' >"$WORK/sign-up.json"
code=$(request POST "$AUTH/api/auth/sign-up/email" \
	-H 'content-type: application/json' -H "origin: $WEB_ORIGIN" \
	--data @"$WORK/sign-up.json")
expect 200 "$code" "sign-up"
USER_ID=$(field '.user.id')

# --- Discovery -------------------------------------------------------------

log "following the 401 challenge from /mcp"
code=$(request POST "$RESOURCE" -H 'content-type: application/json' \
	-H 'accept: application/json, text/event-stream' \
	-H "mcp-protocol-version: $MCP_VERSION" -H 'mcp-method: tools/list' \
	--data '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}')
expect 401 "$code" "unauthenticated MCP request"
challenge=$(header www-authenticate)
METADATA_URL=$(printf '%s' "$challenge" | sed -n 's/.*resource_metadata="\([^"]*\)".*/\1/p')
[ "$METADATA_URL" = "$API/.well-known/oauth-protected-resource/mcp" ] ||
	fail "unexpected resource-metadata challenge"

code=$(request GET "$METADATA_URL")
expect 200 "$code" "protected-resource metadata"
[ "$(field '.resource')" = "$RESOURCE" ] || fail "the metadata names another resource"
ISSUER=$(field '.authorization_servers[0]')
[ "$ISSUER" = "$AUTH" ] || fail "unexpected authorization server"

code=$(request GET "$ISSUER/.well-known/oauth-authorization-server")
expect 200 "$code" "authorization-server metadata"
[ "$(field '.issuer')" = "$ISSUER" ] || fail "the metadata issuer differs"
[ "$(jq -c '.code_challenge_methods_supported' "$BODY")" = '["S256"]' ] ||
	fail "PKCE methods are not exactly S256"
jq -e '.grant_types_supported | index("client_credentials") | not' "$BODY" >/dev/null ||
	fail "client credentials are offered"
AUTHORIZE=$(field '.authorization_endpoint')
TOKEN=$(field '.token_endpoint')
REGISTER=$(field '.registration_endpoint')

# --- Registration ----------------------------------------------------------

# The per-address registration cap starts empty in this disposable database.
log "registering a public client"
jq -nc --arg r "$REDIRECT_URI" '{
		client_name: "AriadShift e2e", redirect_uris: [$r],
		token_endpoint_auth_method: "none",
		grant_types: ["authorization_code", "refresh_token"], response_types: ["code"]
	}' >"$WORK/registration.json"
code=$(request POST "$REGISTER" -H 'content-type: application/json' --data @"$WORK/registration.json")
case $code in 200 | 201) ;; *) expect 201 "$code" "client registration" ;; esac
CLIENT_ID=$(field '.client_id')

VERIFIER=$(openssl rand 48 | base64url)
CHALLENGE=$(printf '%s' "$VERIFIER" | openssl dgst -sha256 -binary | base64url)
STATE=$(random_hex | cut -c1-24)

# authorize [curl args...]: GET the authorization endpoint with the common
# parameters plus the given ones; stores the redirect in $LOCATION.
authorize() {
	code=$(request GET "$AUTHORIZE" -G -b "$JAR" -c "$JAR" \
		--data-urlencode response_type=code \
		--data-urlencode "client_id=$CLIENT_ID" \
		--data-urlencode "redirect_uri=$REDIRECT_URI" \
		--data-urlencode 'scope=openid offline_access' \
		--data-urlencode "state=$STATE" \
		--data-urlencode "resource=$RESOURCE" "$@")
	expect 302 "$code" "authorization request"
	LOCATION=$(header location)
}

# --- PKCE is mandatory -----------------------------------------------------

log "checking that PKCE is required and S256 only"
authorize
[ "$(param "$LOCATION" error)" = invalid_request ] || fail "authorization without PKCE was not refused"
authorize --data-urlencode "code_challenge@$(private_file plain-challenge "$VERIFIER")" --data-urlencode code_challenge_method=plain
[ "$(param "$LOCATION" error)" = invalid_request ] || fail "the plain PKCE method was not refused"
case $LOCATION in "$REDIRECT_URI"*) ;; *) fail "PKCE errors did not return to the client" ;; esac

# --- Authorization ---------------------------------------------------------

log "authorizing with S256 PKCE for $RESOURCE"
authorize --data-urlencode "code_challenge=$CHALLENGE" --data-urlencode code_challenge_method=S256
SIGN_IN_URL=$LOCATION
case $SIGN_IN_URL in "$AUTH/oauth/sign-in?"*) ;; *) fail "not sent to the sign-in page" ;; esac
code=$(request GET "$SIGN_IN_URL")
expect 200 "$code" "sign-in page"
grep -q 'id="password-form"' "$BODY" || fail "the sign-in page has no password form"

log "signing in as the sign-in page does"
jq -nc --arg e "$EMAIL" --arg q "$(query_of "$SIGN_IN_URL")" \
	'{email: $e, password: $ENV.PASSWORD, oauth_query: $q}' >"$WORK/sign-in.json"
code=$(request POST "$AUTH/api/auth/sign-in/email" -b "$JAR" -c "$JAR" \
	-H 'content-type: application/json' -H 'accept: application/json' -H "origin: $AUTH" \
	--data @"$WORK/sign-in.json")
expect 200 "$code" "sign-in"
CONSENT_URL=$(field '.url')
case $CONSENT_URL in "$AUTH/oauth/consent?"*) ;; *) fail "not sent to the consent page" ;; esac
code=$(request GET "$CONSENT_URL")
expect 200 "$code" "consent page"

log "approving as the consent page does"
code=$(request POST "$AUTH/api/auth/oauth2/consent" -b "$JAR" -c "$JAR" \
	-H 'content-type: application/json' -H 'accept: application/json' -H "origin: $AUTH" \
	--data "$(jq -nc --arg q "$(query_of "$CONSENT_URL")" '{accept: true, oauth_query: $q}')")
expect 200 "$code" "consent"
CALLBACK=$(field '.url')
case $CALLBACK in "$REDIRECT_URI?"*) ;; *) fail "consent did not return to the client" ;; esac
[ "$(param "$CALLBACK" state)" = "$STATE" ] || fail "the state was not returned"
[ "$(param "$CALLBACK" iss)" = "$(uri "$ISSUER")" ] || fail "the callback names another issuer"
CODE=$(param "$CALLBACK" code)
[ -n "$CODE" ] || fail "the callback has no code"

# --- Token -----------------------------------------------------------------

log "exchanging the code"
code=$(request POST "$TOKEN" \
	--data-urlencode grant_type=authorization_code \
	--data-urlencode "code@$(private_file code "$CODE")" \
	--data-urlencode "redirect_uri=$REDIRECT_URI" \
	--data-urlencode "client_id=$CLIENT_ID" \
	--data-urlencode "code_verifier@$(private_file verifier "$VERIFIER")" \
	--data-urlencode "resource=$RESOURCE")
expect 200 "$code" "token exchange"
[ "$(field '.token_type')" = Bearer ] || fail "the token is not a Bearer token"
ACCESS_TOKEN=$(field '.access_token')
REFRESH_TOKEN=$(field '.refresh_token')
claims "$ACCESS_TOKEN" | jq -e --arg r "$RESOURCE" --arg i "$ISSUER" --arg s "$USER_ID" '
	(.aud | if type == "array" then . else [.] end | index($r)) != null
	and .iss == $i and .sub == $s and (.exp - .iat) == 900' >/dev/null ||
	fail "the access token's audience, issuer, subject or lifetime is wrong"

# --- MCP over Streamable HTTP ----------------------------------------------

log "calling server/discover and tools/list with the access token"
code=$(mcp "$ACCESS_TOKEN" server/discover)
expect 200 "$code" "MCP server/discover"
jq -e '.error | not' "$BODY" >/dev/null || fail "server/discover failed: $(diagnostic_body)"
code=$(mcp "$ACCESS_TOKEN" tools/list)
expect 200 "$code" "MCP tools/list"
jq -e '[.result.tools[].name] | index("convert") != null' "$BODY" >/dev/null ||
	fail "tools/list does not list convert"

# Use the session's API JWT for REST upload admission, then the resource-bound
# OAuth access token for both MCP conversion and job polling.
log "uploading Markdown and converting it through MCP"
code=$(request GET "$AUTH/api/auth/token" -b "$JAR")
expect 200 "$code" "API JWT for upload"
API_JWT=$(field '.token')
printf '# OAuth conversion\n\nVerified uploaded conversion.\n' >"$WORK/input.md"
SIZE=$(wc -c <"$WORK/input.md" | tr -d ' ')
SHA=$(openssl dgst -sha256 "$WORK/input.md" | awk '{print $NF}')
CHECKSUM=$(openssl dgst -sha256 -binary "$WORK/input.md" | openssl base64 -A)
code=$(request POST "$API/v1/uploads" -H @"$(private_file api-auth "authorization: Bearer $API_JWT")" \
	-H 'content-type: application/json' \
	--data "$(jq -nc --arg s "$SHA" --argjson n "$SIZE" '{filename:"oauth.md",size:$n,sha256:$s}')")
expect 201 "$code" "upload admission"
UPLOAD_ID=$(field '.id')
PUT_URL=$(field '.upload.url')
code=$(request PUT "$PUT_URL" -H "x-amz-checksum-sha256: $CHECKSUM" --data-binary "@$WORK/input.md")
expect 200 "$code" "upload PUT"
code=$(request POST "$API/v1/uploads/$UPLOAD_ID/complete" -H @"$(private_file api-auth "authorization: Bearer $API_JWT")")
expect 200 "$code" "upload completion"
code=$(mcp "$ACCESS_TOKEN" tools/call "$(jq -nc --arg id "$UPLOAD_ID" '{name:"convert",arguments:{upload_id:$id,to:"html"}}')")
expect 200 "$code" "MCP convert"
jq -e '(.error | not) and (.result.isError | not)' "$BODY" >/dev/null || fail "MCP convert failed: $(diagnostic_body)"
JOB_ID=$(field '.result.structuredContent.job.id')
deadline=$(($(date +%s) + 180))
while :; do
	code=$(mcp "$ACCESS_TOKEN" tools/call "$(jq -nc --arg id "$JOB_ID" '{name:"job_status",arguments:{job_id:$id}}')")
	expect 200 "$code" "MCP job_status"
	status=$(field '.result.structuredContent.job.status')
	case $status in
		succeeded) break ;;
		failed | canceled | expired) fail "conversion ended with $status: $(diagnostic_body)" ;;
	esac
	[ "$(date +%s)" -lt "$deadline" ] || fail "conversion did not finish"
	sleep 1
done
OUTPUT_URL=$(field '.result.structuredContent.outputs[0].url')
code=$(request GET "$OUTPUT_URL")
expect 200 "$code" "output download"
grep -q 'Verified uploaded conversion' "$BODY" || fail "converted HTML is missing the input text"

log "refreshing the access token"
code=$(request POST "$TOKEN" \
	--data-urlencode grant_type=refresh_token \
	--data-urlencode "refresh_token@$(private_file refresh "$REFRESH_TOKEN")" \
	--data-urlencode "client_id=$CLIENT_ID" \
	--data-urlencode "resource=$RESOURCE")
expect 200 "$code" "refresh"
REFRESHED_TOKEN=$(field '.access_token')
code=$(mcp "$REFRESHED_TOKEN" tools/list)
expect 200 "$code" "MCP tools/list with the refreshed token"

# --- The ordinary API JWT is not an MCP credential ------------------------

log "checking that the API JWT is refused at /mcp"
code=$(request GET "$AUTH/api/auth/token" -b "$JAR")
expect 200 "$code" "API JWT"
API_JWT=$(field '.token')
claims "$API_JWT" | jq -e --arg a "$AUTH" '.aud == $a' >/dev/null ||
	fail "the API JWT does not carry the auth origin as its audience"
code=$(mcp "$API_JWT" tools/list)
expect 401 "$code" "MCP with the API JWT"
case $(header www-authenticate) in *"resource_metadata=\"$METADATA_URL\""*) ;; *)
	fail "the 401 has no resource-metadata challenge" ;;
esac

# This address has already registered one client. Exercise the real database
# rate limiter after the conversion flow, without consuming its budget first.
log "checking the anonymous registration cap against PostgreSQL"
for ((registration = 1; registration < 20; registration++)); do
	code=$(request POST "$REGISTER" -H 'content-type: application/json' --data @"$WORK/registration.json")
	expect 201 "$code" "registration within the cap"
done
code=$(request POST "$REGISTER" -H 'content-type: application/json' --data @"$WORK/registration.json")
expect 429 "$code" "registration above the cap"
