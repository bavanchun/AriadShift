-- Conversion jobs retain their target; tool jobs use the typed tool registry.
ALTER TABLE jobs ALTER COLUMN target_format DROP NOT NULL;
-- Input-free tools have no upload; the public v1 response uses the nil UUID.
ALTER TABLE jobs ALTER COLUMN upload_id DROP NOT NULL;
ALTER TABLE jobs ADD COLUMN tool_id text;
ALTER TABLE jobs ADD COLUMN params jsonb CHECK (params IS NULL OR jsonb_typeof(params) = 'object');
ALTER TABLE jobs ADD COLUMN tool_plan text NOT NULL DEFAULT 'free'
    CHECK (tool_plan IN ('free', 'pro', 'team', 'enterprise'));
ALTER TABLE jobs ADD CONSTRAINT jobs_operation_check
    CHECK ((tool_id IS NULL AND target_format IS NOT NULL AND upload_id IS NOT NULL)
        OR tool_id IS NOT NULL);
ALTER TABLE jobs DROP CONSTRAINT jobs_pack_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_pack_check
    CHECK (pack IN ('core', 'office', 'docling', 'pdfium', 'qpdf', 'tesseract', 'sign', 'verapdf', 'ai', 'inpaint'));

CREATE TABLE job_inputs (
    job_id uuid NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    position integer NOT NULL CHECK (position >= 0),
    upload_id uuid NOT NULL REFERENCES uploads (id),
    PRIMARY KEY (job_id, position)
);
CREATE INDEX job_inputs_upload_idx ON job_inputs (upload_id);

SELECT pgmq.create('jobs_pdfium');
SELECT pgmq.create('jobs_qpdf');
SELECT pgmq.create('jobs_tesseract');
SELECT pgmq.create('jobs_sign');
SELECT pgmq.create('jobs_verapdf');
SELECT pgmq.create('jobs_ai');
SELECT pgmq.create('jobs_inpaint');

-- Paid accounts keep finite cloud resource limits and higher service quotas.
INSERT INTO plan_entitlements (
    plan, max_input_bytes, max_pages, timeout_s, max_memory_mb, max_asset_bytes,
    max_nesting_depth, max_blocks, max_front_matter_bytes, max_archive_entries,
    max_decompressed_bytes, max_ir_json_bytes,
    jobs_per_day, input_bytes_per_day, concurrent_jobs, requests_per_minute, retention_s
) VALUES ('pro', 2147483648, 5000, 600, 4096, 52428800,
    64, 1000000, 65536, 10000, 2147483648, 3221225472,
    2000, 107374182400, 16, 600, 3600);
