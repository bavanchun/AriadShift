-- Creates the login roles and the Better Auth schema. Runs as the database
-- superuser before any migration, and again on every start; each statement
-- is idempotent and resets passwords to the values in the environment.
--
-- psql variables: api_password, worker_password, auth_password.

\set ON_ERROR_STOP on

-- Runtime logins. They never own a schema, never inherit the superuser and
-- receive table privileges only through grants.sql.
SELECT format('CREATE ROLE %I LOGIN', name)
FROM unnest(ARRAY['ariad_api_login', 'ariad_worker_login', 'ariad_auth']) AS name
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = name)
\gexec

ALTER ROLE ariad_api_login WITH LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 50 PASSWORD :'api_password';
ALTER ROLE ariad_worker_login WITH LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 50 PASSWORD :'worker_password';
ALTER ROLE ariad_auth WITH LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 50 PASSWORD :'auth_password';

-- Only these three logins (and the superuser) may connect to this database.
SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', current_database())
\gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO ariad_api_login, ariad_worker_login, ariad_auth',
    current_database())
\gexec

-- Nobody but the superuser creates objects in public.
REVOKE CREATE ON SCHEMA public FROM PUBLIC;

-- Better Auth owns the auth schema and nothing else. Its tables stay private
-- to it except for the read-only apikey grant in grants.sql.
CREATE SCHEMA IF NOT EXISTS auth AUTHORIZATION ariad_auth;
ALTER SCHEMA auth OWNER TO ariad_auth;
REVOKE ALL ON SCHEMA auth FROM PUBLIC;

-- ariad-server reads Better Auth's "apikey" table unqualified, after its own
-- public tables, so public stays first for the API.
ALTER ROLE ariad_auth SET search_path = auth;
ALTER ROLE ariad_api_login SET search_path = public, auth;
ALTER ROLE ariad_worker_login SET search_path = public;
