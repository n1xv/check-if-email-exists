-- Speeds up the /v1/analytics/blocked endpoint, which filters v1_task_result
-- by created_at over a time period.
CREATE INDEX IF NOT EXISTS idx_v1_task_result_created_at
    ON v1_task_result (created_at);
