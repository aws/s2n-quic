import { create } from "zustand";
import { parseMermaid } from "../parser/mermaid";
import type { MetricKind, TopologyGraph } from "../schema/topology";
import type { AdapterConfig, MetricSeries, QueryStatus } from "../adapters/types";
import {
  toObservableSnapshot,
  type ObservableSnapshot,
} from "../observable/export";

// ---------------------------------------------------------------------------
// Store types
// ---------------------------------------------------------------------------

export type RenderMode = "inline" | "adjacent" | "hover";
export type PerKindMode = "inline" | "adjacent" | "hover" | "hidden";

interface TopologyStore {
  // Topology
  mermaidText: string;
  mermaidUrl: string;
  configUrl: string;
  graph: TopologyGraph | null;
  parseError: string | null;
  setMermaidText(text: string): void;
  setMermaidUrl(url: string): void;
  setConfigUrl(url: string): void;

  // Adapter
  adapterConfig: AdapterConfig;
  setAdapterConfig(cfg: AdapterConfig): void;

  // Metric data: nodeId → MetricKey → MetricSeries
  metricData: Record<string, Record<string, MetricSeries>>;
  // Query status per nodeId
  queryStatus: Record<string, QueryStatus>;
  setMetricData(nodeId: string, series: MetricSeries[]): void;
  setQueryStatus(nodeId: string, status: QueryStatus): void;

  // Display settings
  renderMode: RenderMode;
  setRenderMode(mode: RenderMode): void;
  /** Per-kind mode overrides; absent key means "inherit global renderMode". */
  perKindMode: Partial<Record<MetricKind, PerKindMode>>;
  setPerKindMode(kind: MetricKind, mode: PerKindMode): void;

  // Refresh settings
  refreshIntervalMs: number;
  setRefreshInterval(ms: number): void;
  timeRangeMinutes: number;
  setTimeRange(minutes: number): void;

  // Selection
  selectedNodeId: string | null;
  setSelectedNode(id: string | null): void;

  // Observable export
  exportSnapshot(): ObservableSnapshot;
}

// ---------------------------------------------------------------------------
// Store implementation
// ---------------------------------------------------------------------------

export const useTopologyStore = create<TopologyStore>((set, get) => ({
  // Topology
  mermaidText: "",
  mermaidUrl: "",
  configUrl: "",
  graph: null,
  parseError: null,
  setMermaidText(text) {
    try {
      const graph = parseMermaid(text);
      set({ mermaidText: text, graph, parseError: null });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      set({ mermaidText: text, graph: null, parseError: msg });
    }
  },
  setMermaidUrl(url) {
    set({ mermaidUrl: url });
  },
  setConfigUrl(url) {
    set({ configUrl: url });
  },

  // Adapter
  adapterConfig: { type: "none" },
  setAdapterConfig(cfg) {
    set({ adapterConfig: cfg });
  },

  // Metric data
  metricData: {},
  queryStatus: {},
  setMetricData(nodeId, series) {
    set((state) => {
      const nodeMap: Record<string, MetricSeries> = {
        ...(state.metricData[nodeId] ?? {}),
      };
      for (const s of series) {
        nodeMap[s.key] = s;
      }
      return { metricData: { ...state.metricData, [nodeId]: nodeMap } };
    });
  },
  setQueryStatus(nodeId, status) {
    set((state) => ({
      queryStatus: { ...state.queryStatus, [nodeId]: status },
    }));
  },

  // Display settings
  renderMode: "adjacent",
  setRenderMode(mode) {
    set({ renderMode: mode });
  },
  perKindMode: {},
  setPerKindMode(kind, mode) {
    set((state) => ({
      perKindMode: { ...state.perKindMode, [kind]: mode },
    }));
  },

  // Refresh settings
  refreshIntervalMs: 15_000,
  setRefreshInterval(ms) {
    set({ refreshIntervalMs: ms });
  },
  timeRangeMinutes: 15,
  setTimeRange(minutes) {
    set({ timeRangeMinutes: minutes });
  },

  // Selection
  selectedNodeId: null,
  setSelectedNode(id) {
    set({ selectedNodeId: id });
  },

  // Observable export
  exportSnapshot() {
    const { graph, metricData } = get();
    if (!graph) {
      return {
        schemaVersion: "1" as const,
        capturedAt: new Date().toISOString(),
        graph: { schemaVersion: "1" as const, nodes: [], edges: [] },
        metrics: [],
      };
    }
    return toObservableSnapshot(graph, metricData);
  },
}));
