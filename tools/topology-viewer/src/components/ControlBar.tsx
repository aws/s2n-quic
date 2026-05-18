import { useTopologyStore } from "../store";
import type { AdapterConfig } from "../adapters/types";

const REFRESH_OPTIONS = [
  { label: "5 s", value: 5_000 },
  { label: "15 s", value: 15_000 },
  { label: "30 s", value: 30_000 },
  { label: "1 m", value: 60_000 },
  { label: "5 m", value: 300_000 },
  { label: "Manual", value: 0 },
];

const TIME_RANGE_OPTIONS = [
  { label: "5 m", value: 5 },
  { label: "15 m", value: 15 },
  { label: "1 h", value: 60 },
  { label: "6 h", value: 360 },
  { label: "24 h", value: 1440 },
];

interface Props {
  onRefresh(): void;
}

/**
 * Top control bar: backend selector, Prometheus/CloudWatch inputs,
 * refresh interval, time window, and a manual refresh button.
 */
export function ControlBar({ onRefresh }: Props) {
  const adapterConfig = useTopologyStore((s) => s.adapterConfig);
  const setAdapterConfig = useTopologyStore((s) => s.setAdapterConfig);
  const refreshIntervalMs = useTopologyStore((s) => s.refreshIntervalMs);
  const setRefreshInterval = useTopologyStore((s) => s.setRefreshInterval);
  const timeRangeMinutes = useTopologyStore((s) => s.timeRangeMinutes);
  const setTimeRange = useTopologyStore((s) => s.setTimeRange);

  function handleTypeChange(type: AdapterConfig["type"]) {
    setAdapterConfig({ ...adapterConfig, type });
  }

  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-gray-200 bg-white px-4 py-2 text-sm">
      {/* Backend type */}
      <label className="flex items-center gap-1.5 text-gray-600">
        Backend:
        <select
          className="rounded border border-gray-300 bg-white px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          value={adapterConfig.type}
          onChange={(e) =>
            handleTypeChange(e.target.value as AdapterConfig["type"])
          }
        >
          <option value="none">None</option>
          <option value="prometheus">Prometheus</option>
          <option value="cloudwatch">CloudWatch</option>
        </select>
      </label>

      {/* Prometheus inputs */}
      {adapterConfig.type === "prometheus" && (
        <>
          <input
            type="url"
            placeholder="http://localhost:9090"
            value={adapterConfig.prometheusUrl ?? ""}
            onChange={(e) =>
              setAdapterConfig({ ...adapterConfig, prometheusUrl: e.target.value })
            }
            className="w-56 rounded border border-gray-300 px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          />
          <input
            type="text"
            placeholder="metric prefix (e.g. s2n_quic_dc)"
            value={adapterConfig.prometheusPrefix ?? ""}
            onChange={(e) =>
              setAdapterConfig({
                ...adapterConfig,
                prometheusPrefix: e.target.value,
              })
            }
            className="w-52 rounded border border-gray-300 px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          />
        </>
      )}

      {/* CloudWatch inputs */}
      {adapterConfig.type === "cloudwatch" && (
        <>
          <input
            type="url"
            placeholder="http://localhost:3001 (proxy URL)"
            value={adapterConfig.cloudwatchProxyUrl ?? ""}
            onChange={(e) =>
              setAdapterConfig({
                ...adapterConfig,
                cloudwatchProxyUrl: e.target.value,
              })
            }
            className="w-60 rounded border border-gray-300 px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          />
          <input
            type="text"
            placeholder="Namespace (e.g. s2n-quic-dc)"
            value={adapterConfig.cloudwatchNamespace ?? ""}
            onChange={(e) =>
              setAdapterConfig({
                ...adapterConfig,
                cloudwatchNamespace: e.target.value,
              })
            }
            className="w-48 rounded border border-gray-300 px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          />
        </>
      )}

      <div className="h-5 w-px bg-gray-200" />

      {/* Refresh interval */}
      <label className="flex items-center gap-1.5 text-gray-600">
        Refresh:
        <select
          className="rounded border border-gray-300 bg-white px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          value={refreshIntervalMs}
          onChange={(e) => setRefreshInterval(Number(e.target.value))}
        >
          {REFRESH_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>

      {/* Time range */}
      <label className="flex items-center gap-1.5 text-gray-600">
        Window:
        <select
          className="rounded border border-gray-300 bg-white px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400"
          value={timeRangeMinutes}
          onChange={(e) => setTimeRange(Number(e.target.value))}
        >
          {TIME_RANGE_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>

      <div className="h-5 w-px bg-gray-200" />

      {/* Manual refresh */}
      <button
        onClick={onRefresh}
        className="rounded bg-indigo-600 px-3 py-1 text-white text-sm hover:bg-indigo-700 active:bg-indigo-800 focus:outline-none focus:ring-2 focus:ring-indigo-400"
      >
        ↺ Refresh
      </button>
    </div>
  );
}
