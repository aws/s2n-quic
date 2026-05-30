-- Queue max-depth and backlog comparison across runs.
-- Highlights whether one run had worse queuing behaviour.
CREATE OR REPLACE VIEW compare_queue_health AS
SELECT
    label,
    log_group,
    stream,
    env,
    metric,
    SUM(enq)                AS total_enqueued,
    SUM(drain)              AS total_drained,
    MAX(depth)              AS max_depth,
    ROUND(AVG(depth), 1)    AS avg_depth
FROM runs
WHERE type = 'queue'
GROUP BY label, log_group, stream, env, metric
ORDER BY label, max_depth DESC, log_group, stream, env;
