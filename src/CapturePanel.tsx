import { useEffect, useEffectEvent, useRef, useState } from "react";
import {
  dismissCapturePanel,
  savePanelLink,
  takeCapturePanel,
} from "./capture";
import type { PanelEvent } from "./capture";
import { LinkEditor } from "./LinkEditor";
import { LinkMark, ReceivingCapsule, SymbolIcon } from "./GlassDetails";
import { errorMessage, listCategories } from "./links";
import type { Category, SaveResult } from "./links";
import { usePopupDismiss } from "./popupMotion";
import { usePopupReady } from "./popupReady";

export function CapturePanel() {
  const [event, setEvent] = useState<PanelEvent>();
  const [categories, setCategories] = useState<Category[]>([
    { id: 1, name: "Inbox" },
  ]);
  const [error, setError] = useState("");
  const [committed, setCommitted] = useState<SaveResult>();
  const [presented, setPresented] = useState(false);
  const dismissButton = useRef<HTMLButtonElement>(null);
  const duplicate = committed?.created === false ? committed.link
    : event?.kind === "saved" && !event.result.created ? event.result.link : undefined;

  const { dismiss, finishHandoff, exiting } = usePopupDismiss(
    dismissCapturePanel,
    () => setError("Could not close the panel. Press Alt+F4 to close it."),
  );
  const finishFlight = useEffectEvent(finishHandoff);
  const autoDismiss = useEffectEvent(dismiss);
  const escapeFeedback = useEffectEvent((key: KeyboardEvent) => {
    if (
      (event?.kind === "prompt" && !duplicate) ||
      key.key !== "Escape" ||
      key.isComposing ||
      key.defaultPrevented
    )
      return;
    key.preventDefault();
    dismiss();
  });

  useEffect(() => {
    if (!committed?.created) return;
    if (presented) {
      document.documentElement.setAttribute("data-consuming", "");
      const handoff = setTimeout(
        finishFlight,
        window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
          ? 0
          : 140,
      );
      return () => {
        clearTimeout(handoff);
        document.documentElement.removeAttribute("data-consuming");
      };
    }
    // Text stays available if native presentation failed or its callback was interrupted.
    const deadline = setTimeout(autoDismiss, 2200);
    return () => clearTimeout(deadline);
  }, [committed, presented]);

  useEffect(() => {
    const onKeyDown = (key: KeyboardEvent) => escapeFeedback(key);
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    void takeCapturePanel().then(
      (value) => {
        setEvent(value);
        if (value.kind === "prompt") {
          void listCategories().then(setCategories, (error) =>
            setError(errorMessage(error)),
          );
        }
      },
      (error) => setEvent({ kind: "error", error }),
    );
  }, []);

  useEffect(() => {
    if (event?.kind !== "saved" || !event.result.created) return;
    const timeout = setTimeout(autoDismiss, 2400);
    return () => clearTimeout(timeout);
  }, [event]);

  useEffect(() => {
    if (event?.kind === "error" || duplicate) dismissButton.current?.focus();
  }, [event?.kind, duplicate]);

  usePopupReady(event !== undefined);

  if (event?.kind === "prompt" && !duplicate) {
    return (
      <LinkEditor
        link={null}
        capturedUrl={event.url}
        categories={categories}
        submitDraft={(input) => savePanelLink(input, setPresented)}
        committedMessage={
          committed
            ? committed.created
              ? `Saved to ${committed.link.categoryName}.`
              : "Already saved."
            : ""
        }
        floating
        exiting={exiting}
        onCancel={dismiss}
        onSaved={setCommitted}
        panelError={error}
      />
    );
  }

  return (
    <main className="capture-feedback" data-error={event?.kind === "error" || undefined} data-exiting={exiting || undefined}>
      <header className="glass-heading">
        <h1>Link capture</h1>
        <LinkMark />
      </header>
      {duplicate ? (
        <div className="capture-duplicate" role="status">
          <span className="receiving-capsule"><SymbolIcon name="copy" /> Already saved.</span>
          <strong>{duplicate.title || duplicate.caption || duplicate.url}</strong>
          <span className="hint">{[duplicate.categoryName, duplicate.groupName].filter(Boolean).join(" / ")}</span>
        </div>
      ) : event?.kind === "error" ? (
        <p role="alert" className="error">
          {errorMessage(event.error)}
        </p>
      ) : event?.kind === "saved" ? (
        <ReceivingCapsule
          created={event.result.created}
          category={event.result.link.categoryName}
        />
      ) : (
        <p role="status">Opening capture…</p>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <button
        ref={dismissButton}
        type="button"
        className="danger"
        aria-keyshortcuts="Escape"
        disabled={exiting}
        onClick={dismiss}
      >
        Dismiss
      </button>
    </main>
  );
}
