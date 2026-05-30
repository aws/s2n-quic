-- Top error types ranked by total occurrence count.
-- Shows metric name, variant (if applicable), and aggregate total.
CREATE OR REPLACE VIEW dashboard_errors_top AS
SELECT
    log_group,
    stream,
    env,
    metric,
    variant,
    SUM(CAST(value AS BIGINT))  AS total
FROM metrics
WHERE metric LIKE '!%'
  AND type IN ('nominal', 'scalar')
GROUP BY log_group, stream, env, metric, variant
HAVING total > 0
ORDER BY total DESC, log_group, stream, env, metric
LIMIT 20;
