import type { MetricRegistration } from "../schema/topology";
import type { MetricSeries } from "../adapters/types";

interface Props {
  metrics: MetricRegistration[];
  metricData: Record<string, MetricSeries>;
}

function latestValue(series?: MetricSeries): string {
  if (!series || series.error) return "?";
  const v = series.values.at(-1);
  if (!v) return "—";
  return v.type === "scalar" ? String(v.value) : `p99:${v.p99}`;
}

/**
 * Inline mode: compact vertical list of "label=value" entries rendered as an
 * HTML overlay positioned just below the Mermaid node.
 */
export function MetricInline({ metrics, metricData }: Props) {
  return (
    <div className="flex flex-col gap-0.5 rounded bg-white/90 px-1.5 py-1 text-[10px] font-mono shadow-sm border border-gray-200">
      {metrics.map((m) => {
        const series = metricData[m.key];
        const val = latestValue(series);
        const hasError = !!series?.error;
        const shortName = m.name.replace(/^task\.|^queue\./, "");
        return (
          <span
            key={m.key}
            className={hasError ? "text-red-500" : "text-gray-700"}
          >
            {shortName}
            {m.variant ? `[${m.variant}]` : ""}={val}
          </span>
        );
      })}
    </div>
  );
}
