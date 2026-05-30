-- Queue depth statistics for all internal queues.
-- Shows per-queue enqueue rate, drain rate, and depth (backlog) over the run.
-- A persistently positive depth indicates a consumer is falling behind.
CREATE OR REPLACE VIEW queue_depths AS
SELECT
    log_group,
    stream,
    env,
    metric,
    SUM(enq)                AS total_enqueued,
    SUM(drain)              AS total_drained,
    MAX(depth)              AS max_depth,
    ROUND(AVG(depth), 1)    AS avg_depth
FROM metrics
WHERE type = 'queue'
GROUP BY log_group, stream, env, metric
ORDER BY max_depth DESC, log_group, stream, env, metric;
