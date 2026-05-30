-- RTT and processing latency comparison across runs.
CREATE OR REPLACE VIEW compare_latency AS
SELECT
    label,
    log_group,
    stream,
    env,
    metric,
    SUM(count)  AS samples,
    MAX(p50)    AS p50_us,
    MAX(p99)    AS p99_us,
    MAX(max)    AS max_us
FROM runs
WHERE type = 'histogram'
  AND metric IN (
      'tx.rtt',
      'tx.encrypt_time',
      'rx.decrypt_time',
      'rx.dispatch_time'
  )
GROUP BY label, log_group, stream, env, metric
ORDER BY label, log_group, stream, env, metric;
