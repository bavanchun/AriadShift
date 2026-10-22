-- AriadShift cloud schema.
--
-- Better Auth owns user, session, account, verification, apikey and jwks in
-- the same database. Nothing here references those tables, and this
-- migration never creates or alters them.

CREATE EXTENSION IF NOT EXISTS pgmq;

-- Limits follow ariad_core::Limits (NULL means unlimited); the quota columns
-- are per-day and concurrency caps. Operators tune rows with SQL.
CREATE TABLE plan_entitlements (
    plan text PRIMARY KEY,
    max_input_bytes bigint CHECK (max_input_bytes > 0),
    max_pages integer CHECK (max_pages > 0),
    timeout_s integer CHECK (timeout_s > 0),
    max_memory_mb integer CHECK (max_memory_mb > 0),
    max_asset_bytes bigint CHECK (max_asset_bytes > 0),
    max_nesting_depth integer NOT NULL CHECK (max_nesting_depth BETWEEN 1 AND 99),
    max_blocks integer NOT NULL CHECK (max_blocks > 0),
    max_front_matter_bytes integer NOT NULL CHECK (max_front_matter_bytes > 0),
    max_archive_entries integer NOT NULL CHECK (max_archive_entries > 0),
    max_decompressed_bytes bigint NOT NULL CHECK (max_decompressed_bytes > 0),
    max_ir_json_bytes bigint NOT NULL CHECK (max_ir_json_bytes > 0),
    jobs_per_day integer CHECK (jobs_per_day >= 0),
    input_bytes_per_day bigint CHECK (input_bytes_per_day >= 0),
    concurrent_jobs integer CHECK (concurrent_jobs >= 0),
    requests_per_minute integer CHECK (requests_per_minute >= 0),
    retention_s integer NOT NULL DEFAULT 3600 CHECK (retention_s > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- `free` carries the cloud column of the default limits table; `anonymous`
-- applies to callers without credentials and admits small files only.
INSERT INTO plan_entitlements (
    plan, max_input_bytes, max_pages, timeout_s, max_memory_mb, max_asset_bytes,
    max_nesting_depth, max_blocks, max_front_matter_bytes, max_archive_entries,
    max_decompressed_bytes, max_ir_json_bytes,
    jobs_per_day, input_bytes_per_day, concurrent_jobs, requests_per_minute, retention_s
) VALUES
    ('free', 2147483648, 5000, 600, 4096, 52428800,
     64, 1000000, 65536, 10000,
     2147483648, 3221225472,
     200, 10737418240, 4, 120, 3600),
    ('anonymous', 10485760, 50, 120, 1024, 10485760,
     64, 100000, 65536, 2000,
     268435456, 536870912,
     20, 104857600, 1, 30, 3600);

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    owner_user_id text UNIQUE,
    plan text NOT NULL REFERENCES plan_entitlements (plan),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Shared tenant for anonymous uploads and jobs.
INSERT INTO workspaces (id, owner_user_id, plan)
VALUES ('00000000-0000-0000-0000-000000000000', NULL, 'anonymous');

CREATE TABLE uploads (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id),
    subject text NOT NULL,
    filename text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    format text,
    storage_key text NOT NULL,
    status text NOT NULL CHECK (status IN ('pending', 'completed', 'deleted', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    expires_at timestamptz NOT NULL
);

CREATE INDEX uploads_live_expiry_idx ON uploads (expires_at)
    WHERE status IN ('pending', 'completed');

CREATE TABLE jobs (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces (id),
    subject text NOT NULL,
    upload_id uuid NOT NULL REFERENCES uploads (id),
    source_format text NOT NULL,
    target_format text NOT NULL,
    profile text NOT NULL,
    pack text NOT NULL CHECK (pack IN ('core', 'office', 'docling')),
    status text NOT NULL
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled', 'expired')),
    attempts integer NOT NULL DEFAULT 0,
    limits jsonb NOT NULL,
    warnings jsonb NOT NULL DEFAULT '[]',
    error_code text,
    error_message text,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    expires_at timestamptz NOT NULL
);

CREATE INDEX jobs_workspace_created_idx ON jobs (workspace_id, created_at DESC);
CREATE INDEX jobs_subject_active_idx ON jobs (subject) WHERE status IN ('queued', 'running');
CREATE INDEX jobs_upload_idx ON jobs (upload_id);
CREATE INDEX jobs_live_expiry_idx ON jobs (expires_at) WHERE status <> 'expired';

CREATE TABLE job_steps (
    job_id uuid NOT NULL REFERENCES jobs (id),
    position integer NOT NULL CHECK (position >= 0),
    from_format text NOT NULL,
    to_format text NOT NULL,
    engine text NOT NULL,
    PRIMARY KEY (job_id, position)
);

CREATE TABLE job_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id uuid NOT NULL REFERENCES jobs (id),
    kind text NOT NULL,
    data jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX job_events_job_idx ON job_events (job_id, id);

-- The API streams events over SSE; the notification only names the row, so
-- payloads stay small and the table remains the source of truth.
CREATE FUNCTION ariad_notify_job_event() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify(
        'ariad_job_events',
        json_build_object('job_id', NEW.job_id, 'id', NEW.id)::text
    );
    RETURN NEW;
END
$$;

CREATE TRIGGER job_events_notify
    AFTER INSERT ON job_events
    FOR EACH ROW EXECUTE FUNCTION ariad_notify_job_event();

CREATE TABLE artifacts (
    id uuid PRIMARY KEY,
    job_id uuid NOT NULL REFERENCES jobs (id),
    format text NOT NULL,
    filename text NOT NULL,
    media_type text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    storage_key text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz
);

CREATE INDEX artifacts_job_idx ON artifacts (job_id);

-- Append-only metering ledger. `event_key` makes every write idempotent.
CREATE TABLE usage_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_key text NOT NULL UNIQUE,
    workspace_id uuid NOT NULL,
    subject text NOT NULL,
    job_id uuid,
    kind text NOT NULL CHECK (kind IN ('job_submitted', 'job_completed')),
    input_bytes bigint NOT NULL DEFAULT 0 CHECK (input_bytes >= 0),
    output_bytes bigint NOT NULL DEFAULT 0 CHECK (output_bytes >= 0),
    pages integer NOT NULL DEFAULT 0 CHECK (pages >= 0),
    engine_ms bigint NOT NULL DEFAULT 0 CHECK (engine_ms >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX usage_events_subject_idx ON usage_events (subject, kind, created_at);
CREATE INDEX usage_events_workspace_idx ON usage_events (workspace_id, created_at);

CREATE FUNCTION ariad_reject_usage_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'usage_events is append-only';
END
$$;

CREATE TRIGGER usage_events_append_only
    BEFORE UPDATE OR DELETE ON usage_events
    FOR EACH ROW EXECUTE FUNCTION ariad_reject_usage_change();

CREATE TRIGGER usage_events_no_truncate
    BEFORE TRUNCATE ON usage_events
    FOR EACH STATEMENT EXECUTE FUNCTION ariad_reject_usage_change();

-- Fixed one-minute windows shared by every API replica.
CREATE TABLE rate_limit_counters (
    bucket text NOT NULL,
    window_start timestamptz NOT NULL,
    count integer NOT NULL,
    PRIMARY KEY (bucket, window_start)
);

CREATE INDEX rate_limit_counters_window_idx ON rate_limit_counters (window_start);

-- One queue per engine pack.
SELECT pgmq.create('jobs_core');
SELECT pgmq.create('jobs_office');
SELECT pgmq.create('jobs_docling');
