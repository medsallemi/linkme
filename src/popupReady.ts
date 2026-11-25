import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";

const materialReady = "linkme-popup-material-ready";

export function captureContentHeight(form: HTMLFormElement | null) {
  const dialog = form?.closest("dialog");
  const fields = form?.querySelector("fieldset");
  if (!form || !dialog || !fields?.offsetHeight) return;
  const outerHeight = (element: Element) => {
    const style = getComputedStyle(element);
    return (element as HTMLElement).offsetHeight
      + (parseFloat(style.marginTop) || 0) + (parseFloat(style.marginBottom) || 0);
  };
  const style = getComputedStyle(dialog);
  return Math.max(358, Math.ceil(fields.offsetHeight + dialog.offsetHeight - dialog.clientHeight
    + (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0) + 8
    + Array.from(dialog.children).filter(child => child !== form).reduce((sum, child) => sum + outerHeight(child), 0)
    + Array.from(form.children).slice(1).reduce((sum, child) => sum + outerHeight(child), 0)));
}

export function usePopupReady(ready: boolean) {
  useEffect(() => {
    if (!ready) return;
    const root = document.documentElement;
    function report() {
      if (root.dataset.popupGeneration === undefined) return;
      const generation = Number(root.dataset.popupGeneration);
      window.removeEventListener(materialReady, report);
      const contentHeight = document.body.classList.contains("capture-surface")
        ? captureContentHeight(document.querySelector<HTMLFormElement>("dialog form"))
        : undefined;
      if (contentHeight !== undefined) root.dataset.initialContentHeight = String(contentHeight);
      window.dispatchEvent(new Event("linkme-popup-prepaint"));
      // Hidden WebViews can suspend animation frames.
      root.getBoundingClientRect();
      void invoke("popup_ready", contentHeight === undefined ? { generation } : { generation, contentHeight }).then(() => {
        root.dataset.popupPresented = "true";
        window.dispatchEvent(new Event("linkme-popup-presented"));
      }).catch(() => {
        // The native startup deadline handles failed or missing reports.
      });
    }
    window.addEventListener(materialReady, report);
    report();
    return () => window.removeEventListener(materialReady, report);
  }, [ready]);
}
