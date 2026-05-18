import type {
  DataSourceAdapter,
  MetricSeries,
  TimeRange,
} from "./types";

// ---------------------------------------------------------------------------
// CloudWatch proxy request / response types
// ---------------------------------------------------------------------------

interface CloudWatchDimension {
  Name: string;
  Value: string;
}

interface ProxyRequest {
  namespace: string;
  metricName: string;
  dimensions: CloudWatchDimension[];
  startTime: string; // ISO
  endTime: string;   // ISO
  period: number;    // seconds
  statistics: string[];
}

interface CloudWatchDatapoint {
  Timestamp: string;
  Average?: number;
  Sum?: number;
  SampleCount?: number;
  Minimum?: number;
  Maximum?: number;
  Unit?: string;
}

interface ProxyResponse {
  Datapoints: CloudWatchDatapoint[];
  Label?: string;
}

// ---------------------------------------------------------------------------
// MetricKey → CloudWatch helpers
// ---------------------------------------------------------------------------

function keyToCWMetric(key: string): {
  metricName: string;
  dimensions: CloudWatchDimension[];
} {
  const variantMatch = /\[variant=([^\]]+)\]$/.exec(key);
  const variant = variantMatch ? variantMatch[1] : undefined;
  const metricName = key.replace(/\[.*\]$/, "");
  const dimensions: CloudWatchDimension[] = variant
    ? [{ Name: "variant", Value: variant }]
    : [];
  return { metricName, dimensions };
}

// ---------------------------------------------------------------------------
// CloudWatchAdapter
// ---------------------------------------------------------------------------

/**
 * CloudWatchAdapter sends requests to a developer-supplied HTTP proxy that
 * forwards them to the AWS CloudWatch API. A proxy is required because browsers
 * cannot call AWS APIs directly (CORS restrictions + SigV4 signing).
 *
 * The proxy must accept:
 *   POST {proxyUrl}/metrics
 *   Content-Type: application/json
 *   Body: ProxyRequest
 *
 * And return a JSON body matching ProxyResponse (GetMetricStatistics shape).
 *
 * Minimal Node.js proxy example:
 *   app.post('/metrics', async (req, res) => {
 *     const cw = new CloudWatchClient({ region });
 *     const result = await cw.send(new GetMetricStatisticsCommand(req.body));
 *     res.json(result);
 *   });
 */
export class CloudWatchAdapter implements DataSourceAdapter {
  readonly name = "CloudWatch";

  constructor(
    private readonly proxyUrl: string,
    private readonly namespace: string,
  ) {}

  async ping(): Promise<boolean> {
    try {
      const res = await fetch(`${this.proxyUrl}/health`, {
        signal: AbortSignal.timeout(3_000),
      });
      return res.ok;
    } catch {
      return false;
    }
  }

  async fetchMetrics(
    nodeId: string,
    keys: string[],
    range: TimeRange,
  ): Promise<MetricSeries[]> {
    return Promise.all(
      keys.map((key) => this.fetchSingleKey(nodeId, key, range)),
    );
  }

  private async fetchSingleKey(
    nodeId: string,
    key: string,
    range: TimeRange,
  ): Promise<MetricSeries> {
    const { metricName, dimensions } = keyToCWMetric(key);
    const durationMs = range.end.getTime() - range.start.getTime();
    // Choose a period that yields ≤1440 datapoints (CloudWatch max).
    const period = Math.max(60, Math.ceil(durationMs / (1440 * 1000)));

    const body: ProxyRequest = {
      namespace: this.namespace,
      metricName,
      dimensions,
      startTime: range.start.toISOString(),
      endTime: range.end.toISOString(),
      period,
      statistics: ["Average", "Maximum", "SampleCount"],
    };

    try {
      const res = await fetch(`${this.proxyUrl}/metrics`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(10_000),
      });

      if (!res.ok) {
        return { key, nodeId, values: [], error: `HTTP ${res.status}` };
      }

      const data = (await res.json()) as ProxyResponse;
      const values = (data.Datapoints ?? []).map((dp) => ({
        type: "scalar" as const,
        value: dp.Average ?? dp.Sum ?? 0,
        timestamp: new Date(dp.Timestamp),
      }));

      return { key, nodeId, values };
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { key, nodeId, values: [], error: message };
    }
  }
}

// ---------------------------------------------------------------------------
// NullCloudWatchAdapter — returned when the proxy URL is not configured
// ---------------------------------------------------------------------------

const NOT_CONFIGURED_ERROR = "CloudWatch proxy not configured";

export class NullCloudWatchAdapter implements DataSourceAdapter {
  readonly name = "CloudWatch (unconfigured)";

  async ping(): Promise<boolean> {
    return false;
  }

  async fetchMetrics(
    nodeId: string,
    keys: string[],
  ): Promise<MetricSeries[]> {
    return keys.map((key) => ({
      key,
      nodeId,
      values: [],
      error: NOT_CONFIGURED_ERROR,
    }));
  }
}

/** Return the appropriate CloudWatch adapter depending on proxy URL availability. */
export function makeCloudWatchAdapter(
  proxyUrl: string | undefined,
  namespace: string | undefined,
): DataSourceAdapter {
  if (!proxyUrl) return new NullCloudWatchAdapter();
  return new CloudWatchAdapter(proxyUrl, namespace ?? "s2n-quic-dc");
}
