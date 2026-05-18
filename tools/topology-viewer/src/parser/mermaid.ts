import type {
  MetricKind,
  MetricKey,
  MetricRegistration,
  NodeKind,
  TopologyEdge,
  TopologyGraph,
  TopologyNode,
} from "../schema/topology";

/** Build a canonical MetricKey from a raw name and optional variant. */
export function buildMetricKey(name: string, variant?: string): MetricKey {
  return variant ? `${name}[variant=${variant}]` : name;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

// Node declaration: t0["label text"]
const NODE_RE = /^\s*(\w+)\["([^"]+)"\]\s*$/;
// Class assignment: class t0 task_node;
const CLASS_RE = /^\s*class\s+(\w+)\s+(task_node|channel_node)\s*;?\s*$/;
// Metric comment: %% metric: name [kind opts]: description
const METRIC_RE = /^%%\s+metric:\s+(\S+)\s+\[(\w+)([^\]]*)\]:\s+(.+)$/;
// Subgraph opening: subgraph worker_1[worker 1]
const SUBGRAPH_RE = /^\s*subgraph\s+worker_(\d+)\s*\[/;
// Edge: t0 -->|label| c0
const EDGE_RE = /^\s*(\w+)\s+-->\|([^|]*)\|\s+(\w+)\s*$/;

function parseMetricKind(raw: string): MetricKind | null {
  const allowed: MetricKind[] = ["counter", "gauge", "summary", "timer"];
  return (allowed as string[]).includes(raw) ? (raw as MetricKind) : null;
}

function extractDisplayName(label: string): string {
  // Labels look like:  "task.worker.1<br/>────────<br/>fn: fn<br/>…"
  // The display name is everything before the first <br/> separator.
  const first = label.split(/<br\s*\/?>/i)[0] ?? label;
  return first.replace(/─+/g, "").trim();
}

function parseAttr(attrs: string, key: string): string | undefined {
  const re = new RegExp(`\\b${key}=(\\S+)`);
  const m = re.exec(attrs);
  return m ? m[1] : undefined;
}

function parseEdgeLabel(
  label: string,
): { direction: "sends" | "receives"; fn: string; description: string } {
  // Label format (newlines escaped as \n in Mermaid source):
  //   "sends\nfn: worker_send\nwrite path"
  const lines = label.split(/\\n|\n/).map((l) => l.trim());
  const direction: "sends" | "receives" = lines[0]?.startsWith("receives")
    ? "receives"
    : "sends";
  let fn = "";
  let description = "";
  for (const line of lines.slice(1)) {
    if (line.startsWith("fn:")) {
      fn = line.replace(/^fn:\s*/, "").trim();
    } else if (line) {
      description = line;
    }
  }
  return { direction, fn, description };
}

// ---------------------------------------------------------------------------
// Public parser
// ---------------------------------------------------------------------------

/**
 * Parse Mermaid flowchart text (as emitted by `Topology::to_mermaid()`) into
 * a {@link TopologyGraph}.
 *
 * The parser tracks:
 * - Node declarations and their class assignments (task_node / channel_node)
 * - Subgraph worker_N blocks for workerId assignment
 * - `%% metric:` comments which are attached to the most recently declared node
 * - Edge declarations
 */
export function parseMermaid(text: string): TopologyGraph {
  const nodes: TopologyNode[] = [];
  const edges: TopologyEdge[] = [];

  const nodeById = new Map<string, TopologyNode>();
  let currentWorkerId: number | undefined;
  let lastNodeId: string | null = null;

  for (const line of text.split("\n")) {
    // Subgraph: begin worker group
    const subgraphMatch = SUBGRAPH_RE.exec(line);
    if (subgraphMatch) {
      currentWorkerId = parseInt(subgraphMatch[1], 10);
      continue;
    }

    // End of subgraph
    if (/^\s*end\s*$/.test(line)) {
      currentWorkerId = undefined;
      continue;
    }

    // Node declaration
    const nodeMatch = NODE_RE.exec(line);
    if (nodeMatch) {
      const [, id, label] = nodeMatch;
      const node: TopologyNode = {
        id,
        name: extractDisplayName(label),
        kind: "task", // overridden below by class line
        workerId: currentWorkerId,
        metrics: [],
      };
      nodeById.set(id, node);
      nodes.push(node);
      lastNodeId = id;
      continue;
    }

    // Class assignment
    const classMatch = CLASS_RE.exec(line);
    if (classMatch) {
      const [, id, cls] = classMatch;
      const kind: NodeKind = cls === "task_node" ? "task" : "channel";
      const node = nodeById.get(id);
      if (node) node.kind = kind;
      continue;
    }

    // Metric comment — attach to last seen node id
    const metricMatch = METRIC_RE.exec(line);
    if (metricMatch && lastNodeId) {
      const [, name, kindRaw, attrs, description] = metricMatch;
      const kind = parseMetricKind(kindRaw);
      if (!kind) continue;
      const variant = parseAttr(attrs, "variant");
      const unit = parseAttr(attrs, "unit");
      const reg: MetricRegistration = {
        key: buildMetricKey(name, variant),
        name,
        variant,
        kind,
        unit,
        description: description.trim(),
      };
      nodeById.get(lastNodeId)?.metrics.push(reg);
      continue;
    }

    // Edge — does NOT update lastNodeId so subsequent metric comments still
    // attach to the correct (pre-edge) node.
    const edgeMatch = EDGE_RE.exec(line);
    if (edgeMatch) {
      const [, from, labelRaw, to] = edgeMatch;
      const { direction, fn, description } = parseEdgeLabel(labelRaw);
      edges.push({
        id: `${from}->${to}`,
        from,
        to,
        direction,
        fn,
        description,
      });
    }
  }

  return { schemaVersion: "1", nodes, edges };
}
