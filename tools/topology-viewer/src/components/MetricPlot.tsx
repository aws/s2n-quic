import { useEffect, useMemo, useRef, useState } from "react";
import * as Plot from "@observablehq/plot";
import type { TopologyNode } from "../schema/topology";
import type { MetricSeries } from "../adapters/types";

interface Props {
  node: TopologyNode;
  metricData: Record<string, MetricSeries>;
}

interface ChartPoint {
  timestamp: Date;
  value: number;
  metric: string;
}

function metricLabel(name: string, variant?: string): string {
  return variant ? `${name}[${variant}]` : name;
}

function toPoints(node: TopologyNode, metricData: Record<string, MetricSeries>): ChartPoint[] {
  const points: ChartPoint[] = [];

  for (const metric of node.metrics) {
    const series = metricData[metric.key];
    if (!series || series.error) continue;

    for (const value of series.values) {
      points.push({
        timestamp: value.timestamp,
        value: value.type === "scalar" ? value.value : value.p99,
        metric: metricLabel(metric.name, metric.variant),
      });
    }
  }

  return points;
}

export function MetricPlot({ node, metricData }: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(720);

  const points = useMemo(() => toPoints(node, metricData), [node, metricData]);

  useEffect(() => {
    if (!mountRef.current) return;
    const ro = new ResizeObserver(() => {
      const w = mountRef.current?.clientWidth ?? 720;
      setWidth(Math.max(320, Math.floor(w)));
    });
    ro.observe(mountRef.current);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (!mountRef.current) return;

    mountRef.current.innerHTML = "";
    if (points.length === 0) return;

    const chart = Plot.plot({
      width,
      height: 220,
      marginLeft: 48,
      marginRight: 16,
      marginTop: 10,
      marginBottom: 38,
      x: { label: "Time" },
      y: { label: "Value" },
      color: { legend: true },
      grid: true,
      marks: [
        Plot.line(points, {
          x: "timestamp",
          y: "value",
          stroke: "metric",
          tip: true,
        }),
      ],
    });

    mountRef.current.append(chart);
    return () => {
      chart.remove();
    };
  }, [points, width]);

  if (points.length === 0) {
    return (
      <div className="rounded border border-dashed border-gray-200 bg-gray-50 px-3 py-2 text-xs text-gray-500">
        No metric samples yet.
      </div>
    );
  }

  return <div ref={mountRef} className="w-full overflow-x-auto" />;
}
