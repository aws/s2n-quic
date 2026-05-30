-- Error totals by label, showing which run had more failures.
CREATE OR REPLACE VIEW compare_errors AS
SELECT
    label,
    log_group,
    stream,
    env,
    metric,
    variant,
    SUM(CAST(value AS BIGINT))  AS total
FROM runs
WHERE metric LIKE '!%'
  AND type IN ('nominal', 'scalar')
GROUP BY label, log_group, stream, env, metric, variant
HAVING total > 0
ORDER BY label, log_group, stream, env, total DESC, metric;
