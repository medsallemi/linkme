import { useEffect, useId, useRef } from "react";

import type { ReactNode } from "react";

export function Modal({
  title,
  headingAdornment,
  describedBy,
  onCancel,
  children,
}: {
  title: string;
  headingAdornment?: ReactNode;
  describedBy?: string;
  onCancel: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const dialog = ref.current!;
    const previous = document.activeElement as HTMLElement | null;

    dialog.showModal();
    dialog.querySelector<HTMLElement>("[data-initial-focus]")?.focus();

    return () => {
      dialog.close();
      if (previous?.isConnected && previous !== document.body) previous.focus({ preventScroll: true });
      // A nested dialog must not override focus already restored by its closing parent.
      else if (document.activeElement === document.body)
        document.getElementById("search")?.focus({ preventScroll: true });
      window.dispatchEvent(new Event("linkme-library-ready"));
    };
  }, []);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={describedBy}
      aria-modal="true"
      onKeyDown={(event) => {
        const dialog = event.currentTarget;
        if (event.key !== "Tab" || event.defaultPrevented
            || (event.target as Element).closest("dialog") !== dialog) return;
        const stops = Array.from(dialog.querySelectorAll<HTMLElement>(
          "a[href], button, input, select, textarea, summary, [tabindex]",
        )).filter((item) =>
          item.tabIndex >= 0 && !item.matches(":disabled")
          && item.closest("dialog") === dialog && item.getClientRects().length > 0,
        );
        const radioGroup = (item: HTMLElement) =>
          item instanceof HTMLInputElement && item.type === "radio" && item.name;
        const tabbable = stops.filter((item) => {
          const name = radioGroup(item);
          if (!name) return true;
          const group = stops.filter((control) => radioGroup(control) === name);
          return item === (group.find((control) => (control as HTMLInputElement).checked) || group[0]);
        });
        const first = tabbable[0];
        const last = tabbable.at(-1);
        if (!first || document.activeElement === dialog
            || document.activeElement === (event.shiftKey ? first : last)) {
          event.preventDefault();
          (event.shiftKey ? last : first)?.focus();
        }
      }}
      onCancel={(e) => {
        if (e.target !== e.currentTarget) return;
        e.preventDefault();
        onCancel();
      }}
    >
      {headingAdornment ? (
        <header className="glass-heading">
          <h2 id={titleId}>{title}</h2>
          {headingAdornment}
        </header>
      ) : (
        <h2 id={titleId}>{title}</h2>
      )}
      {children}
    </dialog>
  );
}

export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  cancelLabel = "Cancel",
  pending = false,
  error = "",
  onConfirm,
  onCancel,
}: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  pending?: boolean;
  error?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const descriptionId = useId();
  return (
    <Modal
      title={title}
      describedBy={descriptionId}
      onCancel={() => {
        if (!pending) onCancel();
      }}
    >
      <div id={descriptionId} className="confirmation-copy">{children}</div>

      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}

      <div className="dialog-actions">
        <button
          type="button"
          className="secondary"
          data-initial-focus
          aria-disabled={pending || undefined}
          onClick={() => { if (!pending) onCancel(); }}
        >
          {cancelLabel}
        </button>

        <button
          type="button"
          className="danger"
          aria-disabled={pending || undefined}
          onClick={() => { if (!pending) onConfirm(); }}
        >
          {pending ? "Working…" : confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
