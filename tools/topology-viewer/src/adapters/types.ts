export interface TimeRange {
  start: Date;
  end: Date;
}

export type MetricValue =
  | { type: "scalar"; value: number; timestamp: Date }
  | {
      type: "histogram";
      count: number;
      p50: number;
      p99: number;
      max: number;
      timestamp: Date;
    };

export interface MetricSeries {
  /** The MetricKey this series belongs to. */
  key: string;
  nodeId: string;
  values: MetricValue[];
  /** Present when the query failed for this metric. */
  error?: string;
}

export interface QueryStatus {
  loading: boolean;
  error?: string;
  lastUpdated?: Date;
}

export interface DataSourceAdapter {
  readonly name: string;
  /** Fetch metric values for a list of canonical MetricKeys. */
  fetchMetrics(
    nodeId: string,
    keys: string[],
    range: TimeRange,
  ): Promise<MetricSeries[]>;
  /** Lightweight connectivity check; returns true when reachable. */
  ping(): Promise<boolean>;
}

export interface AdapterConfig {
  type: "prometheus" | "cloudwatch" | "none";
  prometheusUrl?: string;
  prometheusPrefix?: string;
  cloudwatchRegion?: string;
  cloudwatchNamespace?: string;
  /** HTTP proxy URL for CloudWatch (required to avoid browser CORS issues). */
  cloudwatchProxyUrl?: string;
}
