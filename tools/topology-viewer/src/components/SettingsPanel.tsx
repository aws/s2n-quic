import { useTopologyStore, type RenderMode, type PerKindMode } from "../store";
import type { MetricKind } from "../schema/topology";
import { toObservableSnapshot } from "../observable/export";

interface Props {
  open: boolean;
  onClose(): void;
}

const GLOBAL_MODES: { label: string; value: RenderMode }[] = [
  { label: "Inline", value: "inline" },
  { label: "Adjacent", value: "adjacent" },
  { label: "Hover", value: "hover" },
];

const PER_KIND_VALUES: { label: string; value: PerKindMode }[] = [
  { label: "Inline", value: "inline" },
  { label: "Adjacent", value: "adjacent" },
  { label: "Hover", value: "hover" },
  { label: "Hidden", value: "hidden" },
];

const KINDS: MetricKind[] = ["counter", "gauge", "summary", "timer"];

/**
 * Slide-over settings panel (right edge).
 * Controls global render mode, per-kind overrides, and Observable export.
 */
export function SettingsPanel({ open, onClose }: Props) {
  const renderMode = useTopologyStore((s) => s.renderMode);
  const setRenderMode = useTopologyStore((s) => s.setRenderMode);
  const perKindMode = useTopologyStore((s) => s.perKindMode);
  const setPerKindMode = useTopologyStore((s) => s.setPerKindMode);
  const graph = useTopologyStore((s) => s.graph);
  const metricData = useTopologyStore((s) => s.metricData);

  function handleExport() {
    if (!graph) return;
    const snapshot = toObservableSnapshot(graph, metricData);
    const blob = new Blob([JSON.stringify(snapshot, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `topology-snapshot-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div
      className={`fixed right-0 top-0 h-full w-80 transform bg-white shadow-2xl transition-transform duration-200 z-40 flex flex-col ${
        open ? "translate-x-0" : "translate-x-full"
      }`}
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3">
        <h2 className="font-semibold text-gray-800">Settings</h2>
        <button
          onClick={onClose}
          className="text-gray-400 hover:text-gray-600 text-lg leading-none"
          aria-label="Close settings"
        >
          ✕
        </button>
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto p-4 space-y-6">
        {/* Global render mode */}
        <section>
          <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
            Global render mode
          </h3>
          <div className="flex gap-2">
            {GLOBAL_MODES.map((m) => (
              <button
                key={m.value}
                onClick={() => setRenderMode(m.value)}
                className={`flex-1 rounded border py-1.5 text-sm transition-colors ${
                  renderMode === m.value
                    ? "border-indigo-500 bg-indigo-50 text-indigo-700 font-medium"
                    : "border-gray-300 text-gray-600 hover:border-gray-400"
                }`}
              >
                {m.label}
              </button>
            ))}
          </div>
        </section>

        {/* Per-kind overrides */}
        <section>
          <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-500 mb-2">
            Per-kind overrides
          </h3>
          <div className="space-y-2">
            {KINDS.map((kind) => (
              <div key={kind} className="flex items-center justify-between">
                <span className="text-sm text-gray-700 capitalize">{kind}</span>
                <select
                  value={perKindMode[kind] ?? ""}
                  onChange={(e) => {
                    const v = e.target.value;
                    if (v === "") {
                      // Reset to inherit — remove override from perKindMode
                      const next = { ...perKindMode };
                      delete next[kind];
                      useTopologyStore.setState({ perKindMode: next });
                    } else {
                      setPerKindMode(kind, v as PerKindMode);
                    }
                  }}
                  className="rounded border border-gray-300 bg-white px-2 py-1 text-xs focus:outline-none focus:ring-1 focus:ring-indigo-400"
                >
                  <option value="">Inherit</option>
                  {PER_KIND_VALUES.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </div>
            ))}
          </div>
        </section>
      </div>

      {/* Footer */}
      <div className="border-t border-gray-200 p-4">
        <button
          onClick={handleExport}
          disabled={!graph}
          className="w-full rounded bg-indigo-600 px-3 py-2 text-sm text-white hover:bg-indigo-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          ↓ Export snapshot (Observable)
        </button>
      </div>
    </div>
  );
}
