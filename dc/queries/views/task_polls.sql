-- Async-task poll-time and inter-poll latency (p50 / p99 / max) by task name.
-- task.*.time     = wall-clock time spent inside a single poll (µs)
-- task.*.next_poll_latency = time from one poll returning to the next starting (µs)
-- Both metrics are histograms; the variant column carries a per-worker identifier.
CREATE OR REPLACE VIEW task_polls AS
SELECT
    log_group,
    stream,
    env,
    metric,
    SUM(count)              AS total_polls,
    MAX(p50)                AS p50_us,
    MAX(p99)                AS p99_us,
    MAX(max)                AS max_us
FROM metrics
WHERE type = 'histogram'
  AND (metric LIKE 'task.%.time' OR metric LIKE 'task.%.next_poll_latency')
GROUP BY log_group, stream, env, metric
ORDER BY log_group, stream, env, metric;
