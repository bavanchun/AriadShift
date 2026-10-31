-- Rotate pre-completion size checks so one batch cannot starve later uploads.
ALTER TABLE uploads ADD COLUMN size_checked_at timestamptz;
CREATE INDEX uploads_size_check_idx ON uploads (size_checked_at NULLS FIRST, created_at)
    WHERE status IN ('pending', 'deleted');

-- Existing retired records must follow the same filename erasure contract.
UPDATE uploads SET filename = '' WHERE status IN ('deleted', 'expired');
UPDATE artifacts SET filename = '' WHERE deleted_at IS NOT NULL;

-- Shared SSE leases bound open streams across API replicas. Expiry recovers
-- capacity after a process crashes or a disconnected stream cannot be released.
CREATE TABLE event_stream_leases (
    id uuid PRIMARY KEY,
    subject text NOT NULL,
    job_id uuid NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX event_stream_leases_subject_idx ON event_stream_leases (subject, expires_at);
CREATE INDEX event_stream_leases_job_idx ON event_stream_leases (job_id, expires_at);
CREATE INDEX event_stream_leases_expiry_idx ON event_stream_leases (expires_at);
