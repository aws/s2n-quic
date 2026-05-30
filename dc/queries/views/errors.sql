-- All error counters (metrics whose name starts with '!').
-- Includes packet loss, routing asymmetry, decode failures, and more.
-- Any non-zero total warrants investigation.
CREATE OR REPLACE VIEW errors AS
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

UNION ALL

SELECT
    log_group,
    stream,
    env,
    metric,
    NULL                        AS variant,
    SUM(count)                  AS total
FROM metrics
WHERE metric LIKE '!%'
  AND type = 'histogram'
GROUP BY log_group, stream, env, metric
HAVING total > 0

ORDER BY total DESC, log_group, stream, env, metric;
