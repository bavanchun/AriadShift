-- Include completed uploads in the fair rotation while signed PUT URLs live.
DROP INDEX uploads_size_check_idx;
CREATE INDEX uploads_size_check_idx ON uploads (size_checked_at NULLS FIRST, created_at)
    WHERE status IN ('pending', 'completed', 'deleted');
