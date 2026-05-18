import type { MetricSeries } from "../adapters/types";

interface Props {
  metricKey: string;
  series?: MetricSeries;
}

function latestValue(series?: MetricSeries): string {
  if (!series) return "—";
  if (series.error) return "err";
  const v = series.values.at(-1);
  if (!v) return "no data";
  return v.type === "scalar"
    ? v.value.toLocaleString()
    : `p99:${v.p99}`;
}

function statusColor(series?: MetricSeries): string {
  if (!series || series.error) return "bg-red-100 text-red-700 border-red-200";
  if (series.values.length === 0) return "bg-gray-100 text-gray-500 border-gray-200";
  return "bg-indigo-50 text-indigo-700 border-indigo-200";
}

/** Small badge shown in adjacent mode next to a Mermaid node. */
export function MetricBadge({ metricKey, series }: Props) {
  // Show the leaf segment of the key for compactness, e.g. "drained" from
  // "task.ack_burst.drained[variant=recv.0]"
  const shortKey = metricKey.replace(/^.*\./, "").replace(/\[.*\]$/, "");

  return (
    <span
      className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] font-mono leading-none ${statusColor(series)}`}
      title={metricKey}
    >
      <span className="opacity-70">{shortKey}</span>
      <span className="font-semibold">{latestValue(series)}</span>
    </span>
  );
}
