-- Run as the schema owner after AriadShift and Better Auth migrations.
-- Grant these NOLOGIN roles to distinct API/worker login roles; keep the
-- migration owner's login and credentials out of both runtime containers.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ariad_api') THEN
        CREATE ROLE ariad_api NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ariad_worker') THEN
        CREATE ROLE ariad_worker NOLOGIN;
    END IF;
END
$$;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public, pgmq TO ariad_api, ariad_worker;
GRANT SELECT ON _sqlx_migrations, plan_entitlements TO ariad_api, ariad_worker;
GRANT SELECT, INSERT, UPDATE, DELETE ON uploads, jobs, job_steps, job_events,
    artifacts TO ariad_api, ariad_worker;
GRANT SELECT, INSERT, UPDATE, DELETE ON workspaces, rate_limit_counters,
    event_stream_leases TO ariad_api;
REVOKE ALL ON workspaces, rate_limit_counters, event_stream_leases FROM ariad_worker;
GRANT SELECT, INSERT ON usage_events TO ariad_api, ariad_worker;
GRANT USAGE, SELECT ON SEQUENCE job_events_id_seq, usage_events_id_seq
    TO ariad_api, ariad_worker;

-- No grant on Better Auth's user/session/account/jwks tables. Better Auth
-- must not grant those tables to PUBLIC or either runtime login by other roles.
GRANT SELECT ON "apikey" TO ariad_api;

-- pgmq functions are invoker-security functions. Runtime roles need only the
-- existing queues, never extension/schema/queue administration privileges.
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA pgmq FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON pgmq.q_jobs_core, pgmq.a_jobs_core,
    pgmq.q_jobs_office, pgmq.a_jobs_office, pgmq.q_jobs_docling, pgmq.a_jobs_docling
    TO ariad_api, ariad_worker;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA pgmq TO ariad_api, ariad_worker;
GRANT EXECUTE ON FUNCTION pgmq.send(text, jsonb, jsonb),
    pgmq.send(text, jsonb, jsonb, timestamp with time zone),
    pgmq.read(text, integer, integer, jsonb),
    pgmq.read_with_poll(text, integer, integer, integer, integer, jsonb),
    pgmq.set_vt(text, bigint, integer), pgmq.archive(text, bigint),
    pgmq.set_vt(text, bigint, timestamp with time zone),
    pgmq.format_table_name(text, text),
    pgmq.metrics(text) TO ariad_api, ariad_worker;
