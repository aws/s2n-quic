-- Round-trip time and processing latency histograms.
-- tx.rtt: end-to-end measured RTT (µs)
-- tx.encrypt_time / rx.decrypt_time: crypto path latency (µs)
-- rx.dispatch_time: time from receive to application delivery (µs)
CREATE OR REPLACE VIEW rtt AS
SELECT
    log_group,
    stream,
    env,
    metric,
    variant,
    SUM(count)  AS samples,
    MAX(p50)    AS p50_us,
    MAX(p99)    AS p99_us,
    MAX(max)    AS max_us
FROM metrics
WHERE type = 'histogram'
  AND metric IN (
      'tx.rtt',
      'tx.encrypt_time',
      'rx.decrypt_time',
      'rx.dispatch_time'
  )
GROUP BY log_group, stream, env, metric, variant
ORDER BY log_group, stream, env, metric, variant;
