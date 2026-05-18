# topology-viewer

A Vite + React 18 + TypeScript + Tailwind CSS app that visualizes s2n-quic-dc
pipeline topology diagrams with live metric overlays from Prometheus or
CloudWatch.

## Quick start

```bash
cd tools/topology-viewer
npm install
npm run dev
```

Open http://localhost:5173 in your browser.

## Usage

### 1. Load a Mermaid diagram

You can still paste Mermaid text in the textarea, but you can now also load it
from a URL in the control bar (**Load Mermaid URL**). The URL is stored in the
query string as `?mermaid=...`, so the viewer can reopen and fetch it
automatically (for example from gist raw content or a localhost endpoint).

You can also provide a config URL (`?config=...`) that points to a JSON config
document, then click **Load Config URL**.

Supported config fields:

```json
{
  "adapterConfig": {
    "type": "none | prometheus | cloudwatch",
    "prometheusUrl": "http://localhost:9090",
    "prometheusPrefix": "s2n_quic_dc",
    "cloudwatchProxyUrl": "http://localhost:3001",
    "cloudwatchNamespace": "s2n-quic-dc"
  },
  "refreshIntervalMs": 15000,
  "timeRangeMinutes": 15,
  "renderMode": "inline | adjacent | hover",
  "perKindMode": {
    "counter": "inline | adjacent | hover | hidden",
    "gauge": "inline | adjacent | hover | hidden",
    "summary": "inline | adjacent | hover | hidden",
    "timer": "inline | adjacent | hover | hidden"
  },
  "mermaidUrl": "https://example.com/topology.mmd",
  "mermaidText": "flowchart LR\\n..."
}
```

`mermaidUrl` is the typical entry-point value; `mermaidText` is a fallback when
you want to embed source directly in config.

### 2. Configure a metrics backend

Open the **Control Bar** at the top:

| Backend | Configuration |
|---------|--------------|
| **None** | Graph-only mode. No metric fetching. |
| **Prometheus** | Prometheus base URL (e.g. `http://localhost:9090`) and optional metric name prefix (e.g. `s2n_quic_dc`). |
| **CloudWatch** | HTTP proxy URL that forwards requests to the AWS CloudWatch API. The proxy must accept `POST /metrics` with a JSON body and return `GetMetricStatistics` response format. A proxy is required because browsers cannot call AWS APIs directly (CORS + SigV4). |

### 3. Control refresh and time window

Use the **Control Bar** dropdowns:

- **Refresh**: 5 s / 15 s / 30 s / 1 m / 5 m / Manual
- **Window**: 5 m / 15 m / 1 h / 6 h / 24 h
- **↺ Refresh**: force an immediate fetch

### 4. Switch rendering modes

Click the ⚙ (gear) icon on the right edge to open the **Settings** panel:

| Mode | Description |
|------|-------------|
| **Inline** | Metric values appear as a compact overlay just below each node. |
| **Adjacent** | Colour-coded badges appear to the right of each node. |
| **Hover** | Values appear in a popover tooltip when you hover the indicator dot. |

Per-metric-kind overrides (counter / gauge / summary / timer) let you mix modes
or hide noisy metrics entirely.

### 5. Drill down into a node

Click any node to open the **Drilldown Panel** at the bottom. It shows node
metadata, an Observable Plot time-series chart, and a table of all registered
metrics with their latest value, unit, and query status (loading / error /
timestamp).

### 6. Export for Observable

In the Settings panel, click **↓ Export snapshot** to download a JSON file
containing the full topology graph plus a flat table of the latest metric
values. This format is designed to be compatible with Observable notebooks and
Observable Plot.

## Metric key format

The canonical metric key used by adapters is:

```
label[variant=foo]   # when a variant is present
label                # otherwise
```

Example: `task.ack_burst.drained[variant=recv.0]`

**Prometheus mapping**: dots → underscores, prefix prepended, variant → label
selector.  
`s2n_quic_dc_task_ack_burst_drained{variant="recv.0"}`

**CloudWatch mapping**: raw label as MetricName, variant → Dimension
`Name="variant", Value="recv.0"`, namespace from config.

## Development

```bash
npm run typecheck   # type-check without emitting
npm run build       # production build → dist/
npm run preview     # serve the production build locally
```
