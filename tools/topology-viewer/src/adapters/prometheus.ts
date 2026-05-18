import type {
  DataSourceAdapter,
  MetricSeries,
  MetricValue,
  TimeRange,
} from "./types";

// ---------------------------------------------------------------------------
// Prometheus query-range response types (minimal subset we need)
// ---------------------------------------------------------------------------

interface PrometheusResult {
  metric: Record<string, string>;
  values: [number, string][];
}

interface PrometheusQueryRangeResponse {
  status: "success" | "error";
  data?: {
    resultType: string;
    result: PrometheusResult[];
  };
  error?: string;
}

// ---------------------------------------------------------------------------
// MetricKey → PromQL helpers
// ---------------------------------------------------------------------------

/**
 * Derive a Prometheus metric name and optional label selector from a MetricKey.
 *
 * Examples (prefix = "s2n_quic_dc"):
 *   "task.ack_burst.drained[variant=recv.0]"
 *     → "s2n_quic_dc_task_ack_burst_drained{variant="recv.0"}"
 *
 *   "task.ack_burst.drained"
 *     → "s2n_quic_dc_task_ack_burst_drained"
 */
function keyToPromQL(key: string, prefix?: string): string {
  const variantMatch = /\[variant=([^\]]+)\]$/.exec(key);
  const variant = variantMatch ? variantMatch[1] : undefined;
  const baseName = key.replace(/\[.*\]$/, "");

  const metricName = [prefix, baseName.replace(/\./g, "_")]
    .filter(Boolean)
    .join("_");

  const escapedVariant = variant
    ? variant.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")
    : undefined;
  const selector = escapedVariant ? `{variant="${escapedVariant}"}` : "";
  return `${metricName}${selector}`;
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

export class PrometheusAdapter implements DataSourceAdapter {
  readonly name = "Prometheus";

  constructor(
    private readonly baseUrl: string,
    private readonly prefix?: string,
  ) {}

  async ping(): Promise<boolean> {
    try {
      const res = await fetch(`${this.baseUrl}/api/v1/query?query=up`, {
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
    const query = keyToPromQL(key, this.prefix);
    const url = new URL(`${this.baseUrl}/api/v1/query_range`);
    url.searchParams.set("query", query);
    url.searchParams.set("start", range.start.toISOString());
    url.searchParams.set("end", range.end.toISOString());
    url.searchParams.set("step", "60s");

    try {
      const res = await fetch(url.toString(), {
        signal: AbortSignal.timeout(10_000),
      });
      if (!res.ok) {
        return { key, nodeId, values: [], error: `HTTP ${res.status}` };
      }
      const body = (await res.json()) as PrometheusQueryRangeResponse;
      if (body.status !== "success" || !body.data) {
        return {
          key,
          nodeId,
          values: [],
          error: body.error ?? "Prometheus returned non-success status",
        };
      }

      const values: MetricValue[] = body.data.result.flatMap((series) =>
        series.values.map(([ts, val]) => ({
          type: "scalar" as const,
          value: parseFloat(val),
          timestamp: new Date(ts * 1000),
        })),
      );

      return { key, nodeId, values };
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { key, nodeId, values: [], error: message };
    }
  }
}
