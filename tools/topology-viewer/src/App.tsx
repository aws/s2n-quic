import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import { useTopologyStore } from "./store";
import { ControlBar } from "./components/ControlBar";
import { SettingsPanel } from "./components/SettingsPanel";
import { TopologyGraph } from "./components/TopologyGraph";
import { DrilldownPanel } from "./components/DrilldownPanel";
import { PrometheusAdapter } from "./adapters/prometheus";
import { makeCloudWatchAdapter } from "./adapters/cloudwatch";
import type { DataSourceAdapter, TimeRange } from "./adapters/types";

// Sample diagram that demonstrates the Mermaid format produced by Topology::to_mermaid().
const SAMPLE_MERMAID = `flowchart LR
  classDef task_node fill:#eef2ff,stroke:#4f46e5,stroke-width:2px;
  classDef channel_node fill:#ecfeff,stroke:#0891b2,stroke-width:2px;
  subgraph worker_1[worker 1]
    t0["task.ack_burst<br/>────────<br/>fn: endpoint::Worker::spawn<br/>budget: 256<br/>metrics: tracked<br/>desc: Encodes and submits ACK bursts from recv contexts"]
    class t0 task_node;
%% metric: task.ack_burst.drained [summary variant=recv.0 unit=count]: Number of items processed per poll for this task
%% metric: task.ack_burst.next_poll_latency [timer variant=recv.0 unit=microsecond]: Wall-clock latency between consecutive task polls
%% metric: task.ack_burst.time [timer variant=recv.0 unit=microsecond]: Wall-clock duration spent inside each task poll
  end
  c0["queue.ack_burst.recv.0<br/>────────<br/>fn: queue_fn<br/>metrics: none<br/>desc: ACK burst queue"]
  class c0 channel_node;
  t0 -->|sends\\nfn: worker_send\\nwrite path| c0
  c0 -->|receives\\nfn: worker_recv\\nread path| t0`;

function buildAdapter(config: {
  type: string;
  prometheusUrl?: string;
  prometheusPrefix?: string;
  cloudwatchProxyUrl?: string;
  cloudwatchNamespace?: string;
}): DataSourceAdapter | null {
  if (config.type === "prometheus" && config.prometheusUrl) {
    return new PrometheusAdapter(config.prometheusUrl, config.prometheusPrefix);
  }
  if (config.type === "cloudwatch") {
    return makeCloudWatchAdapter(
      config.cloudwatchProxyUrl,
      config.cloudwatchNamespace,
    );
  }
  return null;
}

export default function App() {
  const mermaidText = useTopologyStore((s) => s.mermaidText);
  const setMermaidText = useTopologyStore((s) => s.setMermaidText);
  const graph = useTopologyStore((s) => s.graph);
  const adapterConfig = useTopologyStore((s) => s.adapterConfig);
  const setMetricData = useTopologyStore((s) => s.setMetricData);
  const setQueryStatus = useTopologyStore((s) => s.setQueryStatus);
  const refreshIntervalMs = useTopologyStore((s) => s.refreshIntervalMs);
  const timeRangeMinutes = useTopologyStore((s) => s.timeRangeMinutes);
  const parseError = useTopologyStore((s) => s.parseError);

  const [settingsOpen, setSettingsOpen] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Memoize the adapter so it only rebuilds when the relevant config fields
  // change — not on every keystroke in unrelated inputs.
  const adapter = useMemo<DataSourceAdapter | null>(
    () =>
      buildAdapter({
        type: adapterConfig.type,
        prometheusUrl: adapterConfig.prometheusUrl,
        prometheusPrefix: adapterConfig.prometheusPrefix,
        cloudwatchProxyUrl: adapterConfig.cloudwatchProxyUrl,
        cloudwatchNamespace: adapterConfig.cloudwatchNamespace,
      }),
    [
      adapterConfig.type,
      adapterConfig.prometheusUrl,
      adapterConfig.prometheusPrefix,
      adapterConfig.cloudwatchProxyUrl,
      adapterConfig.cloudwatchNamespace,
    ],
  );

  const fetchAll = useCallback(async () => {
    if (!graph || graph.nodes.length === 0) return;
    if (!adapter) return;

    const now = new Date();
    const range: TimeRange = {
      start: new Date(now.getTime() - timeRangeMinutes * 60 * 1000),
      end: now,
    };

    await Promise.all(
      graph.nodes
        .filter((n) => n.metrics.length > 0)
        .map(async (node) => {
          setQueryStatus(node.id, { loading: true });
          try {
            const keys = node.metrics.map((m) => m.key);
            const series = await adapter.fetchMetrics(node.id, keys, range);
            setMetricData(node.id, series);
            setQueryStatus(node.id, {
              loading: false,
              lastUpdated: new Date(),
            });
          } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            setQueryStatus(node.id, { loading: false, error: msg });
          }
        }),
    );
  }, [graph, adapter, timeRangeMinutes, setMetricData, setQueryStatus]);

  // Auto-refresh loop
  useEffect(() => {
    if (intervalRef.current) clearInterval(intervalRef.current);
    if (refreshIntervalMs > 0) {
      intervalRef.current = setInterval(() => {
        void fetchAll();
      }, refreshIntervalMs);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchAll, refreshIntervalMs]);

  // Fetch whenever adapter config or graph changes
  useEffect(() => {
    void fetchAll();
  }, [fetchAll]);

  // Load sample diagram on mount
  useEffect(() => {
    setMermaidText(SAMPLE_MERMAID);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="flex h-screen flex-col bg-gray-50 font-sans text-gray-900">
      {/* Control bar */}
      <ControlBar onRefresh={() => void fetchAll()} />

      {/* Main area */}
      <div className="flex flex-1 overflow-hidden">
        {/* Graph + drilldown */}
        <div className="relative flex flex-1 flex-col overflow-hidden">
          {/* Diagram */}
          <div className="relative flex-1 overflow-hidden">
            <TopologyGraph />
          </div>

          {/* Parse error banner */}
          {parseError && (
            <div className="border-t border-red-200 bg-red-50 px-4 py-1.5 text-xs text-red-600">
              Parse error: {parseError}
            </div>
          )}

          {/* Drilldown panel */}
          <DrilldownPanel />

          {/* Mermaid source editor */}
          <div className="border-t border-gray-200 bg-white">
            <div className="flex items-center justify-between px-3 py-1 text-xs text-gray-500">
              <span>Mermaid diagram source</span>
              <button
                onClick={() => setMermaidText("")}
                className="text-gray-400 hover:text-gray-600"
              >
                Clear
              </button>
            </div>
            <textarea
              className="w-full resize-none border-0 border-t border-gray-100 px-3 py-2 font-mono text-xs text-gray-700 focus:outline-none focus:ring-0 bg-gray-50"
              rows={6}
              placeholder="Paste Mermaid flowchart text here…"
              value={mermaidText}
              onChange={(e) => setMermaidText(e.target.value)}
              spellCheck={false}
            />
          </div>
        </div>

        {/* Settings toggle button */}
        <div className="flex flex-col border-l border-gray-200 bg-white">
          <button
            onClick={() => setSettingsOpen((v) => !v)}
            className="px-3 py-3 text-gray-500 hover:text-indigo-600 hover:bg-gray-50 transition-colors"
            title="Settings"
            aria-label="Toggle settings panel"
          >
            ⚙
          </button>
        </div>
      </div>

      {/* Settings slide-over */}
      <SettingsPanel open={settingsOpen} onClose={() => setSettingsOpen(false)} />

      {/* Modal backdrop */}
      {settingsOpen && (
        <div
          className="fixed inset-0 z-30 bg-black/20"
          onClick={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
