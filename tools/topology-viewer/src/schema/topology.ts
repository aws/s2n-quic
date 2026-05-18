// Frozen v1 contract types — these mirror the Rust TopologyGraph structure.
// Do not change existing field names or types without bumping SCHEMA_VERSION.

export const SCHEMA_VERSION = "1" as const;

/**
 * Canonical metric lookup key used by all adapters.
 * Format: "label[variant=foo]" when a variant is present, else just "label".
 * Example: "task.ack_burst.drained[variant=recv.0]"
 */
export type MetricKey = string;

export type MetricKind = "counter" | "gauge" | "summary" | "timer";

export interface MetricRegistration {
  /** Canonical key used by adapters, e.g. "task.ack_burst.drained[variant=recv.0]" */
  key: MetricKey;
  /** Raw label as it appears in the Mermaid comment, e.g. "task.ack_burst.drained" */
  name: string;
  /** Variant selector, e.g. "recv.0" */
  variant?: string;
  kind: MetricKind;
  /** Physical unit, e.g. "count", "microsecond" */
  unit?: string;
  description: string;
}

export type NodeKind = "task" | "channel";

export interface TopologyNode {
  /** Mermaid node id, e.g. "t0", "c0" */
  id: string;
  /** Display name extracted from the node label, e.g. "task.ack_burst" */
  name: string;
  kind: NodeKind;
  workerId?: number;
  metrics: MetricRegistration[];
}

export interface TopologyEdge {
  /** Synthetic id: "{from}->{to}" */
  id: string;
  from: string;
  to: string;
  direction: "sends" | "receives";
  description: string;
  fn: string;
}

export interface TopologyGraph {
  schemaVersion: "1";
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}
