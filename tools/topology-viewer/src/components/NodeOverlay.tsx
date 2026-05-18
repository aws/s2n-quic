import { useState } from "react";
import type { TopologyNode } from "../schema/topology";
import type { MetricSeries } from "../adapters/types";
import { useTopologyStore } from "../store";
import { MetricBadge } from "./MetricBadge";
import { MetricTooltip } from "./MetricTooltip";
import { MetricInline } from "./MetricInline";

interface Props {
  node: TopologyNode;
  metricData: Record<string, MetricSeries>;
  style?: React.CSSProperties;
}

/**
 * HTML overlay container positioned over / beside a Mermaid node.
 * Delegates rendering to the appropriate sub-component based on the effective
 * render mode for each metric kind.
 */
export function NodeOverlay({ node, metricData, style }: Props) {
  const renderMode = useTopologyStore((s) => s.renderMode);
  const perKindMode = useTopologyStore((s) => s.perKindMode);
  const [hovered, setHovered] = useState(false);

  if (node.metrics.length === 0) return null;

  function effectiveMode(kind: string): string {
    return (perKindMode as Record<string, string>)[kind] ?? renderMode;
  }

  const visibleMetrics = node.metrics.filter(
    (m) => effectiveMode(m.kind) !== "hidden",
  );

  if (visibleMetrics.length === 0) return null;

  const adjacentMetrics = visibleMetrics.filter(
    (m) => effectiveMode(m.kind) === "adjacent",
  );
  const inlineMetrics = visibleMetrics.filter(
    (m) => effectiveMode(m.kind) === "inline",
  );
  const hasHover = visibleMetrics.some(
    (m) => effectiveMode(m.kind) === "hover",
  );
  const hoverMetrics = visibleMetrics.filter(
    (m) => effectiveMode(m.kind) === "hover",
  );

  return (
    <div
      className="pointer-events-auto absolute flex flex-col gap-1"
      style={style}
    >
      {/* Inline overlay */}
      {inlineMetrics.length > 0 && (
        <MetricInline metrics={inlineMetrics} metricData={metricData} />
      )}

      {/* Adjacent badges */}
      {adjacentMetrics.length > 0 && (
        <div className="flex flex-col gap-0.5">
          {adjacentMetrics.map((m) => (
            <MetricBadge key={m.key} metricKey={m.key} series={metricData[m.key]} />
          ))}
        </div>
      )}

      {/* Hover indicator + tooltip */}
      {hasHover && (
        <div
          className="relative"
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
        >
          <div className="h-2.5 w-2.5 rounded-full bg-indigo-400 cursor-pointer ring-2 ring-white shadow" />
          {hovered && (
            <div className="absolute left-4 top-0 z-50">
              <MetricTooltip metrics={hoverMetrics} metricData={metricData} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
