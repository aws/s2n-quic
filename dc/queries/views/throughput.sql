-- Socket-level TX/RX throughput over time.
-- Each row is one endpoint-second bucket aggregating all observations in that window.
-- tx_gbps / rx_gbps are derived from the raw byte totals.
CREATE OR REPLACE VIEW throughput AS
SELECT
    log_group,
    stream,
    env,
    date_trunc('second', to_timestamp(ts)) AS second,
    SUM(bytes) FILTER (WHERE metric = 'socket.tx.bytes') AS tx_bytes,
    SUM(bytes) FILTER (WHERE metric = 'socket.rx.bytes') AS rx_bytes,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.tx.bytes') * 8.0 / 1e9, 3) AS tx_gbps,
    ROUND(SUM(bytes) FILTER (WHERE metric = 'socket.rx.bytes') * 8.0 / 1e9, 3) AS rx_gbps
FROM metrics
WHERE type = 'throughput'
  AND metric IN ('socket.tx.bytes', 'socket.rx.bytes')
GROUP BY log_group, stream, env, second
ORDER BY log_group, stream, env, second;
