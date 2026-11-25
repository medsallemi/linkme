import "./palette.css";
import "./flight.css";
import { playScene, type Scene } from "./flight";
import { symbolPaths } from "./symbols";

declare global {
  interface Window {
    linkmeScene?: Scene;
  }
}
document.documentElement.classList.add("flight-surface");
document.querySelector("#link-symbol")!.setAttribute("d", symbolPaths.link);
document.querySelector("#check-symbol")!.setAttribute("d", symbolPaths.check);
let dispose: (() => void) | undefined;
function start() {
  const scene = window.linkmeScene;
  if (!scene) return;
  delete window.linkmeScene;
  dispose?.();
  dispose = playScene(
    scene,
    document.documentElement,
    document.getElementById("traveler")!,
    document.getElementById("island")!,
    matchMedia("(prefers-reduced-motion: reduce)"),
  );
}
window.addEventListener("linkme-flight", start);
window.addEventListener("pagehide", () => dispose?.(), { once: true });
start();
