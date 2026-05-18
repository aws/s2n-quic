import { useTopologyStore } from "../store";
import type { MetricSeries } from "../adapters/types";

function formatValue(series?: MetricSeries): string {
  if (!series) return "—";
  if (series.error) return `err: ${series.error}`;
  const latest = series.values.at(-1);
  if (!latest) return "no data";
  if (latest.type === "scalar") return latest.value.toLocaleString();
  return `count:${latest.count} p50:${latest.p50} p99:${latest.p99} max:${latest.max}`;
}

function formatTimestamp(series?: MetricSeries): string {
  const latest = series?.values.at(-1);
  if (!latest) return "";
  return latest.timestamp.toLocaleTimeString();
}

/**
 * Sliding panel revealed at the bottom when a node is selected.
 * Shows metadata and a full metric table.
 */
export function DrilldownPanel() {
  const selectedNodeId = useTopologyStore((s) => s.selectedNodeId);
  const setSelectedNode = useTopologyStore((s) => s.setSelectedNode);
  const graph = useTopologyStore((s) => s.graph);
  const metricData = useTopologyStore((s) => s.metricData);
  const queryStatus = useTopologyStore((s) => s.queryStatus);

  if (!selectedNodeId) return null;
  const node = graph?.nodes.find((n) => n.id === selectedNodeId);
  if (!node) return null;

  const nodeMetrics: Record<string, MetricSeries> = metricData[node.id] ?? {};
  const status = queryStatus[node.id];

  return (
    <div className="border-t border-gray-200 bg-white shadow-inner max-h-60 overflow-auto">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-gray-100 sticky top-0 bg-white z-10">
        <div className="flex items-center gap-3 flex-wrap">
          <span className="font-semibold text-gray-800 text-sm">{node.name}</span>
          <span
            className={`text-xs rounded px-1.5 py-0.5 font-medium ${
              node.kind === "task"
                ? "bg-indigo-50 text-indigo-700"
                : "bg-cyan-50 text-cyan-700"
            }`}
          >
            {node.kind}
          </span>
          {node.workerId !== undefined && (
            <span className="text-xs text-gray-400">worker {node.workerId}</span>
          )}
          {status?.loading && (
            <span className="text-xs text-gray-400 animate-pulse">loading…</span>
          )}
          {status?.error && (
            <span className="text-xs text-red-500">{status.error}</span>
          )}
          {status?.lastUpdated && !status.loading && (
            <span className="text-xs text-gray-400">
              updated {status.lastUpdated.toLocaleTimeString()}
            </span>
          )}
        </div>
        <button
          onClick={() => setSelectedNode(null)}
          className="text-gray-400 hover:text-gray-600 text-base leading-none ml-2"
          aria-label="Close drilldown"
        >
          ✕
        </button>
      </div>

      {/* Metrics table */}
      {node.metrics.length === 0 ? (
        <p className="px-4 py-3 text-sm text-gray-400">
          No metrics registered for this node.
        </p>
      ) : (
        <table className="w-full text-xs">
          <thead>
            <tr className="bg-gray-50 text-gray-500 uppercase tracking-wide text-[10px]">
              <th className="px-3 py-2 text-left font-medium">Metric</th>
              <th className="px-3 py-2 text-left font-medium">Kind</th>
              <th className="px-3 py-2 text-left font-medium">Unit</th>
              <th className="px-3 py-2 text-left font-medium">Latest value</th>
              <th className="px-3 py-2 text-left font-medium">Time</th>
              <th className="px-3 py-2 text-left font-medium">Description</th>
            </tr>
          </thead>
          <tbody>
            {node.metrics.map((m, i) => {
              const series = nodeMetrics[m.key];
              const hasError = !!series?.error;
              return (
                <tr
                  key={m.key}
                  className={`border-t border-gray-100 ${
                    i % 2 === 0 ? "bg-white" : "bg-gray-50"
                  }`}
                >
                  <td className="px-3 py-1.5 font-mono text-gray-700 whitespace-nowrap">
                    {m.name}
                    {m.variant && (
                      <span className="ml-1 text-gray-400">[{m.variant}]</span>
                    )}
                  </td>
                  <td className="px-3 py-1.5 text-indigo-600">{m.kind}</td>
                  <td className="px-3 py-1.5 text-gray-500">{m.unit ?? "—"}</td>
                  <td
                    className={`px-3 py-1.5 font-mono whitespace-nowrap ${
                      hasError ? "text-red-500" : "text-gray-800"
                    }`}
                  >
                    {formatValue(series)}
                  </td>
                  <td className="px-3 py-1.5 text-gray-400 whitespace-nowrap">
                    {formatTimestamp(series)}
                  </td>
                  <td
                    className="px-3 py-1.5 text-gray-500 max-w-xs truncate"
                    title={m.description}
                  >
                    {m.description}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
