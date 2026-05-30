-- Single-row summary per label covering throughput, latency, and errors.
-- Best used as the first view when comparing two benchmark runs.
CREATE OR REPLACE VIEW compare_summary AS
SELECT
    label,
    log_group,
    stream,
    env,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.tx.bytes' AND type = 'throughput') * 8.0 / 1e9, 3)  AS total_tx_gbps,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.rx.bytes' AND type = 'throughput') * 8.0 / 1e9, 3)  AS total_rx_gbps,
    MAX(p50) FILTER (WHERE metric = 'tx.rtt' AND type = 'histogram')                                     AS rtt_p50_us,
    MAX(p99) FILTER (WHERE metric = 'tx.rtt' AND type = 'histogram')                                     AS rtt_p99_us,
    SUM(CAST(value AS BIGINT)) FILTER (WHERE metric LIKE '!%' AND type = 'nominal')                      AS total_errors
FROM runs
GROUP BY label, log_group, stream, env
ORDER BY label, log_group, stream, env;
