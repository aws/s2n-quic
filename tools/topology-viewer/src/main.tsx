import React from "react";
import ReactDOM from "react-dom/client";
import mermaid from "mermaid";
import App from "./App";
import "./index.css";

mermaid.initialize({
  startOnLoad: false,
  theme: "default",
  flowchart: { useMaxWidth: false, htmlLabels: true },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
