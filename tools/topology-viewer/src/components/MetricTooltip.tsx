import type { MetricRegistration } from "../schema/topology";
import type { MetricSeries } from "../adapters/types";

interface Props {
  metrics: MetricRegistration[];
  metricData: Record<string, MetricSeries>;
}

function latestValue(series?: MetricSeries): string {
  if (!series) return "—";
  if (series.error) return `err: ${series.error.slice(0, 30)}`;
  const v = series.values.at(-1);
  if (!v) return "no data";
  return v.type === "scalar"
    ? v.value.toLocaleString()
    : `count:${v.count} p99:${v.p99}`;
}

/**
 * Tooltip shown in hover mode when the user mouses over a node indicator dot.
 */
export function MetricTooltip({ metrics, metricData }: Props) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-3 shadow-xl min-w-[220px] text-xs">
      {metrics.map((m) => {
        const series = metricData[m.key];
        const hasError = !!series?.error;
        return (
          <div key={m.key} className="flex items-baseline justify-between gap-3 py-0.5">
            <span className="font-mono text-gray-700 truncate" title={m.name}>
              {m.name.replace(/^task\.|^queue\./, "")}
              {m.variant && (
                <span className="text-gray-400 ml-0.5">[{m.variant}]</span>
              )}
            </span>
            <span
              className={`font-semibold whitespace-nowrap ${
                hasError ? "text-red-500" : "text-indigo-700"
              }`}
            >
              {latestValue(series)}
            </span>
          </div>
        );
      })}
    </div>
  );
}
