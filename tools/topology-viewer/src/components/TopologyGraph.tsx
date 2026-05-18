import { useEffect, useRef, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import mermaid from "mermaid";
import { useTopologyStore } from "../store";
import { NodeOverlay } from "./NodeOverlay";
import type { MetricSeries } from "../adapters/types";

interface NodePosition {
  nodeId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

let renderCounter = 0;

/**
 * Main graph component.
 *
 * 1. Renders the Mermaid diagram into an SVG via `mermaid.render()`.
 * 2. After painting, measures the bounding box of each node element.
 * 3. Injects {@link NodeOverlay} components via React portals, positioned at
 *    the right edge of each node.
 * 4. Re-measures on container resize via ResizeObserver.
 */
export function TopologyGraph() {
  const mermaidText = useTopologyStore((s) => s.mermaidText);
  const graph = useTopologyStore((s) => s.graph);
  const metricData = useTopologyStore((s) => s.metricData);
  const setSelectedNode = useTopologyStore((s) => s.setSelectedNode);

  const containerRef = useRef<HTMLDivElement>(null);
  const [svgContent, setSvgContent] = useState("");
  const [nodePositions, setNodePositions] = useState<NodePosition[]>([]);
  const [renderError, setRenderError] = useState<string | null>(null);

  // Render mermaid SVG whenever the diagram text changes.
  useEffect(() => {
    if (!mermaidText.trim()) {
      setSvgContent("");
      setNodePositions([]);
      return;
    }

    const id = `topology-graph-${++renderCounter}`;
    let cancelled = false;

    mermaid
      .render(id, mermaidText)
      .then(({ svg }) => {
        if (!cancelled) {
          setSvgContent(svg);
          setRenderError(null);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setRenderError(err instanceof Error ? err.message : String(err));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [mermaidText]);

  // Measure node DOM positions after SVG is painted.
  const measureNodes = useCallback(() => {
    if (!containerRef.current || !graph) return;
    const containerRect = containerRef.current.getBoundingClientRect();
    const positions: NodePosition[] = [];

    for (const node of graph.nodes) {
      // Mermaid renders nodes inside <g> elements with an id like
      // "flowchart-t0-1" or "flowchart-t0".
      const el =
        containerRef.current.querySelector(
          `[id^="flowchart-${node.id}-"]`,
        ) ??
        containerRef.current.querySelector(`[id="flowchart-${node.id}"]`) ??
        containerRef.current.querySelector(`g.node#${node.id}`);

      if (!el) continue;
      const rect = el.getBoundingClientRect();
      positions.push({
        nodeId: node.id,
        x: rect.left - containerRect.left,
        y: rect.top - containerRect.top,
        width: rect.width,
        height: rect.height,
      });
    }
    setNodePositions(positions);
  }, [graph]);

  useEffect(() => {
    if (!svgContent) return;
    // One rAF gives the browser time to paint the injected SVG.
    const raf = requestAnimationFrame(measureNodes);
    return () => cancelAnimationFrame(raf);
  }, [svgContent, measureNodes]);

  // Re-measure when the container resizes (e.g. panel open/close).
  useEffect(() => {
    if (!containerRef.current) return;
    const ro = new ResizeObserver(measureNodes);
    ro.observe(containerRef.current);
    return () => ro.disconnect();
  }, [measureNodes]);

  // Click on SVG to select a node.
  const handleNodeClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!graph) return;
      let el: Element | null = e.target as Element;
      while (el && el !== containerRef.current) {
        const id = el.getAttribute("id") ?? "";
        const match = /flowchart-(\w+)(?:-\d+)?$/.exec(id);
        if (match) {
          const nodeId = match[1];
          if (graph.nodes.some((n) => n.id === nodeId)) {
            setSelectedNode(nodeId);
            return;
          }
        }
        el = el.parentElement;
      }
    },
    [graph, setSelectedNode],
  );

  if (!mermaidText.trim()) {
    return (
      <div className="flex h-full items-center justify-center text-gray-400 text-sm">
        Paste Mermaid topology text below to render the diagram.
      </div>
    );
  }

  if (renderError) {
    return (
      <div className="p-4 text-red-600 text-sm font-mono bg-red-50 rounded border border-red-200">
        <p className="font-semibold mb-1">Mermaid render error</p>
        <pre className="whitespace-pre-wrap">{renderError}</pre>
      </div>
    );
  }

  return (
    <div
      className="relative h-full w-full overflow-auto"
      ref={containerRef}
      onClick={handleNodeClick}
    >
      {svgContent && (
        <div
          // biome-ignore lint/security/noDangerouslySetInnerHtml: SVG is from mermaid library
          dangerouslySetInnerHTML={{ __html: svgContent }}
        />
      )}

      {/* Metric overlays via React portals anchored inside the container. */}
      {containerRef.current !== null &&
        nodePositions.map((pos) => {
          const node = graph?.nodes.find((n) => n.id === pos.nodeId);
          if (!node || node.metrics.length === 0) return null;
          const nodeMetrics: Record<string, MetricSeries> =
            metricData[node.id] ?? {};
          return createPortal(
            <NodeOverlay
              key={node.id}
              node={node}
              metricData={nodeMetrics}
              style={{
                position: "absolute",
                left: pos.x + pos.width + 4,
                top: pos.y,
              }}
            />,
            // containerRef.current is narrowed to non-null above
            containerRef.current as Element,
          );
        })}
    </div>
  );
}
