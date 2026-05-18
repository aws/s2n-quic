import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import { useTopologyStore, type RenderMode, type PerKindMode } from "./store";
import { ControlBar } from "./components/ControlBar";
import { SettingsPanel } from "./components/SettingsPanel";
import { TopologyGraph } from "./components/TopologyGraph";
import { DrilldownPanel } from "./components/DrilldownPanel";
import { PrometheusAdapter } from "./adapters/prometheus";
import { makeCloudWatchAdapter } from "./adapters/cloudwatch";
import type { AdapterConfig, DataSourceAdapter, TimeRange } from "./adapters/types";
import type { MetricKind } from "./schema/topology";

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

interface RemoteViewerConfig {
  adapterConfig?: AdapterConfig;
  refreshIntervalMs?: number;
  timeRangeMinutes?: number;
  renderMode?: RenderMode;
  perKindMode?: Partial<Record<MetricKind, PerKindMode>>;
  mermaidText?: string;
  mermaidUrl?: string;
}

const VALID_ADAPTER_TYPES = ["none", "prometheus", "cloudwatch"] as const;

function isAdapterType(value: unknown): value is AdapterConfig["type"] {
  return (
    typeof value === "string" &&
    (VALID_ADAPTER_TYPES as readonly string[]).includes(value)
  );
}

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

function getErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function cleanUrlInput(value: string): string {
  return value.trim();
}

async function fetchText(url: string): Promise<string> {
  const resp = await fetch(url);
  if (!resp.ok) {
    throw new Error(`HTTP ${resp.status} while fetching ${url}`);
  }
  return await resp.text();
}

async function fetchJson(url: string): Promise<unknown> {
  const resp = await fetch(url);
  if (!resp.ok) {
    throw new Error(`HTTP ${resp.status} while fetching ${url}`);
  }
  return await resp.json();
}

export default function App() {
  const mermaidText = useTopologyStore((s) => s.mermaidText);
  const setMermaidText = useTopologyStore((s) => s.setMermaidText);
  const mermaidUrl = useTopologyStore((s) => s.mermaidUrl);
  const setMermaidUrl = useTopologyStore((s) => s.setMermaidUrl);
  const configUrl = useTopologyStore((s) => s.configUrl);
  const setConfigUrl = useTopologyStore((s) => s.setConfigUrl);
  const graph = useTopologyStore((s) => s.graph);
  const adapterConfig = useTopologyStore((s) => s.adapterConfig);
  const setAdapterConfig = useTopologyStore((s) => s.setAdapterConfig);
  const setMetricData = useTopologyStore((s) => s.setMetricData);
  const setQueryStatus = useTopologyStore((s) => s.setQueryStatus);
  const refreshIntervalMs = useTopologyStore((s) => s.refreshIntervalMs);
  const setRefreshInterval = useTopologyStore((s) => s.setRefreshInterval);
  const timeRangeMinutes = useTopologyStore((s) => s.timeRangeMinutes);
  const setTimeRange = useTopologyStore((s) => s.setTimeRange);
  const parseError = useTopologyStore((s) => s.parseError);
  const setRenderMode = useTopologyStore((s) => s.setRenderMode);

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sourceLoadError, setSourceLoadError] = useState<string | null>(null);
  const [sourceLoadStatus, setSourceLoadStatus] = useState<string | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const bootstrapRanRef = useRef(false);

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
            const msg = getErrorMessage(err);
            setQueryStatus(node.id, { loading: false, error: msg });
          }
        }),
    );
  }, [graph, adapter, timeRangeMinutes, setMetricData, setQueryStatus]);

  const loadMermaidFromUrl = useCallback(
    async (url: string) => {
      const cleanUrl = cleanUrlInput(url);
      if (!cleanUrl) return;
      setSourceLoadStatus(`Loading Mermaid from ${cleanUrl}...`);
      setSourceLoadError(null);
      try {
        const text = await fetchText(cleanUrl);
        setMermaidText(text);
        setSourceLoadStatus(`Loaded Mermaid from ${cleanUrl}`);
      } catch (err) {
        const msg = getErrorMessage(err);
        setSourceLoadError(msg);
        setSourceLoadStatus(null);
      }
    },
    [setMermaidText],
  );

  const applyConfigObject = useCallback(
    (raw: unknown): RemoteViewerConfig => {
      if (!raw || typeof raw !== "object") {
        throw new Error("Config response must be a JSON object.");
      }

      const cfg = raw as Record<string, unknown>;
      const next: RemoteViewerConfig = {};

      if (cfg.adapterConfig && typeof cfg.adapterConfig === "object") {
        const adapter = cfg.adapterConfig as Record<string, unknown>;
        if (!isAdapterType(adapter.type)) {
          throw new Error(
            `Invalid adapterConfig.type "${String(adapter.type)}". Expected one of: ${VALID_ADAPTER_TYPES.join(", ")}.`,
          );
        }
        next.adapterConfig = {
          type: adapter.type,
          prometheusUrl:
            typeof adapter.prometheusUrl === "string"
              ? adapter.prometheusUrl
              : undefined,
          prometheusPrefix:
            typeof adapter.prometheusPrefix === "string"
              ? adapter.prometheusPrefix
              : undefined,
          cloudwatchProxyUrl:
            typeof adapter.cloudwatchProxyUrl === "string"
              ? adapter.cloudwatchProxyUrl
              : undefined,
          cloudwatchNamespace:
            typeof adapter.cloudwatchNamespace === "string"
              ? adapter.cloudwatchNamespace
              : undefined,
          cloudwatchRegion:
            typeof adapter.cloudwatchRegion === "string"
              ? adapter.cloudwatchRegion
              : undefined,
        };
      }

      if (typeof cfg.refreshIntervalMs === "number") {
        next.refreshIntervalMs = cfg.refreshIntervalMs;
      }
      if (typeof cfg.timeRangeMinutes === "number") {
        next.timeRangeMinutes = cfg.timeRangeMinutes;
      }
      if (
        cfg.renderMode === "inline" ||
        cfg.renderMode === "adjacent" ||
        cfg.renderMode === "hover"
      ) {
        next.renderMode = cfg.renderMode;
      }

      if (typeof cfg.mermaidText === "string") {
        next.mermaidText = cfg.mermaidText;
      }
      if (typeof cfg.mermaidUrl === "string") {
        next.mermaidUrl = cfg.mermaidUrl;
      }

      if (cfg.perKindMode && typeof cfg.perKindMode === "object") {
        const nextPerKind: Partial<Record<MetricKind, PerKindMode>> = {};
        const perKind = cfg.perKindMode as Record<string, unknown>;
        for (const kind of ["counter", "gauge", "summary", "timer"] as const) {
          const mode = perKind[kind];
          if (
            mode === "inline" ||
            mode === "adjacent" ||
            mode === "hover" ||
            mode === "hidden"
          ) {
            nextPerKind[kind] = mode;
          }
        }
        next.perKindMode = nextPerKind;
      }

      return next;
    },
    [],
  );

  const loadConfigFromUrl = useCallback(
    async (url: string, overrideMermaidUrl?: string) => {
      const cleanUrl = cleanUrlInput(url);
      if (!cleanUrl) return;
      setSourceLoadStatus(`Loading config from ${cleanUrl}...`);
      setSourceLoadError(null);
      try {
        const raw = await fetchJson(cleanUrl);
        const cfg = applyConfigObject(raw);

        if (cfg.adapterConfig) setAdapterConfig(cfg.adapterConfig);
        if (typeof cfg.refreshIntervalMs === "number") {
          setRefreshInterval(cfg.refreshIntervalMs);
        }
        if (typeof cfg.timeRangeMinutes === "number") {
          setTimeRange(cfg.timeRangeMinutes);
        }
        if (cfg.renderMode) {
          setRenderMode(cfg.renderMode);
        }
        if (cfg.perKindMode) {
          useTopologyStore.setState({ perKindMode: cfg.perKindMode });
        }

        const currentMermaidUrl = useTopologyStore.getState().mermaidUrl;
        const effectiveMermaidUrl = cleanUrlInput(
          overrideMermaidUrl ?? currentMermaidUrl,
        );
        const configMermaidUrl = cleanUrlInput(cfg.mermaidUrl ?? "");

        if (effectiveMermaidUrl) {
          await loadMermaidFromUrl(effectiveMermaidUrl);
        } else if (configMermaidUrl) {
          setMermaidUrl(configMermaidUrl);
          await loadMermaidFromUrl(configMermaidUrl);
        } else if (cfg.mermaidText) {
          setMermaidText(cfg.mermaidText);
        }

        setSourceLoadStatus(`Loaded config from ${cleanUrl}`);
      } catch (err) {
        const msg = getErrorMessage(err);
        setSourceLoadError(msg);
        setSourceLoadStatus(null);
      }
    },
    [
      applyConfigObject,
      loadMermaidFromUrl,
      setAdapterConfig,
      setMermaidText,
      setMermaidUrl,
      setRefreshInterval,
      setRenderMode,
      setTimeRange,
    ],
  );

  // Keep query params in sync with source links.
  useEffect(() => {
    if (!bootstrapRanRef.current) return;

    const params = new URLSearchParams(window.location.search);
    const nextMermaidUrl = cleanUrlInput(mermaidUrl);
    const nextConfigUrl = cleanUrlInput(configUrl);

    if (nextMermaidUrl) params.set("mermaid", nextMermaidUrl);
    else params.delete("mermaid");

    if (nextConfigUrl) params.set("config", nextConfigUrl);
    else params.delete("config");

    const query = params.toString();
    const nextUrl = `${window.location.pathname}${query ? `?${query}` : ""}${window.location.hash}`;
    window.history.replaceState(null, "", nextUrl);
  }, [mermaidUrl, configUrl]);

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

  // Initial load from query params (config + mermaid links), fallback to sample.
  useEffect(() => {
    let cancelled = false;

    async function loadInitial() {
      if (bootstrapRanRef.current) return;
      try {
        const params = new URLSearchParams(window.location.search);
        const configFromQuery = cleanUrlInput(params.get("config") ?? "");
        const mermaidFromQuery = cleanUrlInput(params.get("mermaid") ?? "");

        if (configFromQuery) setConfigUrl(configFromQuery);
        if (mermaidFromQuery) setMermaidUrl(mermaidFromQuery);

        if (configFromQuery) {
          await loadConfigFromUrl(configFromQuery, mermaidFromQuery);
          return;
        }

        if (mermaidFromQuery) {
          await loadMermaidFromUrl(mermaidFromQuery);
          return;
        }

        if (!cancelled) {
          setMermaidText(SAMPLE_MERMAID);
        }
      } finally {
        if (!cancelled) {
          bootstrapRanRef.current = true;
        }
      }
    }

    void loadInitial();
    return () => {
      cancelled = true;
    };
  }, [loadConfigFromUrl, loadMermaidFromUrl, setConfigUrl, setMermaidText, setMermaidUrl]);

  return (
    <div className="flex h-screen flex-col bg-gray-50 font-sans text-gray-900">
      {/* Control bar */}
      <ControlBar
        onRefresh={() => void fetchAll()}
        onLoadMermaidUrl={() => void loadMermaidFromUrl(mermaidUrl)}
        onLoadConfigUrl={() => void loadConfigFromUrl(configUrl, mermaidUrl)}
      />

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

          {/* Source load status */}
          {sourceLoadStatus && (
            <div className="border-t border-indigo-200 bg-indigo-50 px-4 py-1.5 text-xs text-indigo-700">
              {sourceLoadStatus}
            </div>
          )}
          {sourceLoadError && (
            <div className="border-t border-red-200 bg-red-50 px-4 py-1.5 text-xs text-red-600">
              Source load error: {sourceLoadError}
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
