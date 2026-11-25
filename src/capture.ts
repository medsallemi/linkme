import { Channel, invoke } from "@tauri-apps/api/core";
import type { LinkDraft, SaveResult } from "./links";

export interface CaptureSettings {
  mode: "prompt" | "quick";
  shortcut: string;
  searchShortcut: string;
  dimPercent: number;
  panelTransparency: number;
  highlight: number;
}

export interface CaptureConnection {
  settings: CaptureSettings;
  warning: string | null;
  showSettings?: boolean;
}

export type PanelEvent =
  | { kind: "prompt"; url: string }
  | { kind: "saved"; result: SaveResult }
  | { kind: "error"; error: unknown };

export type CaptureEvent = PanelEvent | { kind: "settings" };

export async function connectCapture(receive: (event: CaptureEvent) => void) {
  const events = new Channel<CaptureEvent>(receive);
  return invoke<CaptureConnection>("connect_capture", { events });
}

export function resetLinkme() {
  return invoke<void>("reset_linkme");
}

export function updateCapture(settings: CaptureSettings) {
  return invoke<CaptureConnection>("update_capture", { settings });
}

export function takeCapturePanel() {
  return invoke<PanelEvent>("take_capture_panel");
}

export function dismissCapturePanel() {
  return invoke<void>("dismiss_capture_panel");
}

export function savePanelLink(
  input: LinkDraft,
  onPresentation: (shown: boolean) => void,
) {
  const chip = document
    .querySelector(".capture-surface dialog")
    ?.getBoundingClientRect();
  const anchor = chip
    ? [chip.x + chip.width / 2, chip.y + chip.height / 2]
    : null;
  return invoke<SaveResult>("save_panel_link", {
    input,
    anchor,
    presentation: new Channel<boolean>(onPresentation),
  });
}

export function resizeCapturePanel(expanded: boolean, contentHeight?: number) {
  return invoke<void>("resize_capture_panel", {
    expanded,
    contentHeight,
    animate:
      window.matchMedia?.("(prefers-reduced-motion: no-preference)").matches ??
      false,
  });
}
