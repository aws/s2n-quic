-- High-level health overview: throughput summary and error totals.
-- Use this as the first stop when reviewing a run.
CREATE OR REPLACE VIEW dashboard_overview AS
SELECT
    log_group,
    stream,
    env,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.tx.bytes') * 8.0 / 1e9, 3) AS total_tx_gbps,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.rx.bytes') * 8.0 / 1e9, 3) AS total_rx_gbps,
    SUM(CAST(value AS BIGINT)) FILTER (WHERE metric LIKE '!%' AND type = 'nominal') AS total_errors,
    COUNT(DISTINCT metric) FILTER (WHERE metric LIKE '!%') AS distinct_error_types,
    MIN(to_timestamp(ts)) AS run_start,
    MAX(to_timestamp(ts)) AS run_end
FROM metrics
WHERE type IN ('throughput', 'nominal')
GROUP BY log_group, stream, env
ORDER BY log_group, stream, env;
