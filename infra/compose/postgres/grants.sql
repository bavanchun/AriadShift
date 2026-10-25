-- Applies the server's least-privilege grants and binds them to the runtime
-- logins. Runs as the database superuser after the ariad-server migrations and
-- after Better Auth has created its tables, and again on every start, so new
-- tables, sequences and queue functions are granted after an upgrade.

\set ON_ERROR_STOP on

-- server-roles.sql names Better Auth's "apikey" without a schema.
SET search_path = public, auth;

\i /sql/server-roles.sql

GRANT USAGE ON SCHEMA auth TO ariad_api;
GRANT ariad_api TO ariad_api_login;
GRANT ariad_worker TO ariad_worker_login;

-- A login holds exactly one runtime role.
REVOKE ariad_worker FROM ariad_api_login;
REVOKE ariad_api FROM ariad_worker_login;
