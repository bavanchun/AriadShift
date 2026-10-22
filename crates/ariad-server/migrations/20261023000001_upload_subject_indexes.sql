-- Upload admission counts abandoned uploads independently of conversion usage.
CREATE INDEX uploads_subject_created_idx ON uploads (subject, created_at);
CREATE INDEX uploads_pending_subject_idx ON uploads (subject) WHERE status = 'pending';
