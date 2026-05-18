import type { MetricSeries } from "../adapters/types";
import type { TopologyGraph } from "../schema/topology";

export interface ObservableMetricRow {
  nodeId: string;
  nodeName: string;
  metricKey: string;
  kind: string;
  unit?: string;
  /** Latest scalar value, or p99 for histogram series. */
  value?: number;
  timestamp?: string; // ISO 8601
  error?: string;
}

export interface ObservableSnapshot {
  schemaVersion: "1";
  capturedAt: string; // ISO 8601
  /** Full topology graph for programmatic access. */
  graph: TopologyGraph;
  /** Flat table of latest metric values — suitable for Observable Plot / notebooks. */
  metrics: ObservableMetricRow[];
}

/**
 * Build a plain-object snapshot from live store state.
 * The result is JSON-serialisable and Observable-compatible.
 */
export function toObservableSnapshot(
  graph: TopologyGraph,
  metricData: Record<string, Record<string, MetricSeries>>,
): ObservableSnapshot {
  const rows: ObservableMetricRow[] = [];

  for (const node of graph.nodes) {
    const nodeMetrics = metricData[node.id] ?? {};
    for (const reg of node.metrics) {
      const series = nodeMetrics[reg.key];
      const latest = series?.values.at(-1);
      const row: ObservableMetricRow = {
        nodeId: node.id,
        nodeName: node.name,
        metricKey: reg.key,
        kind: reg.kind,
        unit: reg.unit,
        error: series?.error,
      };
      if (latest) {
        row.value = latest.type === "scalar" ? latest.value : latest.p99;
        row.timestamp = latest.timestamp.toISOString();
      }
      rows.push(row);
    }
  }

  return {
    schemaVersion: "1",
    capturedAt: new Date().toISOString(),
    graph,
    metrics: rows,
  };
}
