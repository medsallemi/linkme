import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { WaterField } from "./WaterField";
import App from "./App";
import { CapturePanel } from "./CapturePanel";
import { SearchOverlay } from "./SearchOverlay";
import { ErrorBoundary } from "./ErrorBoundary";
import "./styles.css";
import "./palette.css";
import "./glass.css";
import "./liquid.css";

const label = isTauri() ? getCurrentWebviewWindow().label : "main";
const capture = label === "capture";
const floating = capture || label === "search";
if (floating)
  document.documentElement.classList.add("floating-glass", "capture-glass");
if (capture) document.body.classList.add("capture-surface");

createRoot(document.getElementById("root")!).render(
  <ErrorBoundary>
    <WaterField glass={floating} />
    {capture ? (
      <CapturePanel />
    ) : label === "search" ? (
      <SearchOverlay />
    ) : (
      <App />
    )}
  </ErrorBoundary>,
);
