-- TX/RX throughput comparison across runs.
-- Each row is one (label, endpoint, second) pair so runs can be plotted side-by-side.
CREATE OR REPLACE VIEW compare_throughput AS
SELECT
    label,
    log_group,
    stream,
    env,
    date_trunc('second', to_timestamp(ts))                                         AS second,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.tx.bytes') * 8.0 / 1e9, 3)   AS tx_gbps,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.rx.bytes') * 8.0 / 1e9, 3)   AS rx_gbps
FROM runs
WHERE type = 'throughput'
  AND metric IN ('socket.tx.bytes', 'socket.rx.bytes')
GROUP BY label, log_group, stream, env, second
ORDER BY label, log_group, stream, env, second;
