import { useEffect, useEffectEvent, useRef, useState } from "react";

import type { FormEvent } from "react";

import { errorMessage, saveLink } from "./links";

import type { Category, Link, LinkDraft, SaveResult } from "./links";

import { ConfirmDialog, Modal } from "./Modal";
import { resizeCapturePanel } from "./capture";
import { IconPicker } from "./CategoryEditor";
import { LinkChip, SymbolIcon } from "./GlassDetails";
import { captureContentHeight } from "./popupReady";

export function LinkEditor({
  link,
  categories,
  focusCategory = false,
  capturedUrl = "",
  submitDraft = saveLink,
  floating = false,
  exiting = false,
  panelError = "",
  committedMessage = "",
  onSaved,
  onCancel,
}: {
  link: Link | null;
  categories: Category[];
  focusCategory?: boolean;
  capturedUrl?: string;
  submitDraft?: (draft: LinkDraft) => Promise<SaveResult>;
  floating?: boolean;
  exiting?: boolean;
  panelError?: string;
  committedMessage?: string;
  onSaved: (result: SaveResult, edited: boolean) => void;
  onCancel: () => void;
}) {
  const [initial] = useState<LinkDraft>(() => ({
    id: link?.id ?? null,
    revision: link?.revision ?? null,
    url: link?.url ?? capturedUrl,
    title: link?.title ?? "",
    subtitle: link?.subtitle ?? "",
    caption: link?.caption ?? "",
    categoryId: link?.categoryId ?? 1,
    groupId: link?.groupId ?? null,
    newCategory: null,
  }));

  const [draft, setDraft] = useState(initial);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);

  const [error, setError] = useState("");
  const [titleError, setTitleError] = useState(false);
  const titleField = useRef<HTMLInputElement>(null);
  const [discard, setDiscard] = useState(false);
  const [resizing, setResizing] = useState(false);
  const resizeRequest = useRef(0);
  const lastSize = useRef({ expanded: false, height: 0 });
  const formRef = useRef<HTMLFormElement>(null);
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const categoryName = useRef<HTMLInputElement>(null);
  const creatingCategory = draft.newCategory !== null;
  const groups = categories.find(category => category.id === draft.categoryId)?.groups ?? [];

  useEffect(() => {
    if (creatingCategory) categoryName.current?.focus({ preventScroll: true });
  }, [creatingCategory]);
  const errorRef = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);

  async function resizeDetails(expanded: boolean) {
    if (!floating || exiting || committedMessage || document.documentElement.dataset.popupPresented !== "true") return;
    const height = captureContentHeight(formRef.current) ?? 0;
    if (!expanded && !lastSize.current.expanded && !lastSize.current.height &&
      height === Number(document.documentElement.dataset.initialContentHeight ?? 0)) return;
    if (lastSize.current.expanded === expanded && lastSize.current.height === height) return;
    lastSize.current = { expanded, height };
    const request = ++resizeRequest.current;
    setResizing(true);
    try {
      if (height) await resizeCapturePanel(expanded, height);
      else await resizeCapturePanel(expanded);
    } catch {
      if (resizeRequest.current === request) {
        setError("Could not resize capture. Details remain scrollable.");
      }
    } finally {
      if (resizeRequest.current === request) setResizing(false);
    }
  }

  const fitPanel = useEffectEvent(() => {
    void resizeDetails(creatingCategory || !!detailsRef.current?.open);
  });

  useEffect(() => {
    const form = formRef.current;
    const fields = form?.querySelector("fieldset");
    if (!floating || !form || !fields || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(fitPanel);
    observer.observe(fields);
    observer.observe(form.children[1] ?? form);
    window.addEventListener("linkme-popup-presented", fitPanel);
    return () => {
      observer.disconnect();
      window.removeEventListener("linkme-popup-presented", fitPanel);
    };
  }, [floating]);

  useEffect(() => {
    if (floating) fitPanel();
  }, [floating, error, titleError, panelError]);

  function change<K extends keyof LinkDraft>(key: K, value: LinkDraft[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
    if (key === "title" && String(value).trim()) setTitleError(false);
    setError("");
  }

  function cancel() {
    if (busy.current || exiting) return;
    if (!floating && JSON.stringify(draft) !== JSON.stringify(initial))
      setDiscard(true);
    else onCancel();
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy.current || exiting || committedMessage) return;
    if (floating && !draft.title.trim()) {
      setError("");
      setTitleError(true);
      titleField.current?.focus();
      return;
    }
    busy.current = true;
    setPending(true);
    setError("");

    try {
      const result = await submitDraft(draft);
      onSaved(result, link !== null);
    } catch (error: unknown) {
      setError(errorMessage(error));
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  const details = (
    <>
      <label htmlFor="edit-subtitle">Subtitle (optional)</label>
      <input
        id="edit-subtitle"
        name="subtitle"
        autoComplete="off"
        value={draft.subtitle}
        onChange={(e) => change("subtitle", e.target.value)}
      />
      <label htmlFor="edit-caption">Caption (optional)</label>
      <textarea
        id="edit-caption"
        name="caption"
        rows={3}
        value={draft.caption}
        onChange={(e) => change("caption", e.target.value)}
      />
    </>
  );

  return (
    <>
      <Modal
        title={link ? "Edit link" : "Add link"}
        headingAdornment={
          floating && (
            <div className="capture-heading-actions">
              <LinkChip url={draft.url} />
              <button
                type="button"
                className="capture-close"
                aria-label="Close capture"
                disabled={pending || exiting}
                onClick={cancel}
              >
                <SymbolIcon name="close" />
              </button>
            </div>
          )
        }
        onCancel={cancel}
      >
        <form
          ref={formRef}
          data-exiting={exiting || undefined}
          noValidate
          onSubmit={(e) => {
            void submit(e);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && e.ctrlKey) {
              e.preventDefault();
              e.currentTarget.requestSubmit();
            }
          }}
        >
          <div className="editor-fields" data-resizing={resizing || undefined}>
            <fieldset disabled={pending || exiting || !!committedMessage}>
              <label htmlFor="edit-url">URL</label>

              <textarea
                id="edit-url"
                name="url"
                rows={2}
                data-initial-focus={
                  (!focusCategory && !(floating && capturedUrl)) || undefined
                }
                value={draft.url}
                spellCheck={false}
                autoComplete="off"
                onChange={(e) => change("url", e.target.value)}
              />

              <label htmlFor="edit-category">Category</label>

              <select
                id="edit-category"
                name="category"
                data-initial-focus={focusCategory || undefined}
                value={
                  draft.newCategory !== null
                    ? "__new__"
                    : String(draft.categoryId)
                }
                onChange={(e) => {
                  const value = e.target.value;
                  setDraft((d) => ({
                    ...d,
                    newCategory: value === "__new__" ? "" : null,
                    groupId: null,
                    categoryId:
                      value === "__new__" ? d.categoryId : Number(value),
                  }));
                  setError("");
                  if (value === "__new__" && detailsRef.current) detailsRef.current.open = true;
                  void resizeDetails(value === "__new__" || !!detailsRef.current?.open);
                }}
              >
                {categories.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
                <option value="__new__">New category…</option>
              </select>

              {!creatingCategory && groups.length > 0 && (
                <div className="editor-group-field">
                  <label htmlFor="edit-group">Group (optional)</label>
                  <select id="edit-group" value={draft.groupId ?? ""} onChange={e => change("groupId", e.target.value ? Number(e.target.value) : null)}>
                    <option value="">No group</option>
                    {groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}
                  </select>
                </div>
              )}
              {creatingCategory && (
                <div className="editor-new-category">
                  <label htmlFor="new-category">New category name</label>
                  <input
                    ref={categoryName}
                    id="new-category"
                    name="newCategory"
                    autoComplete="off"
                    maxLength={80}
                    placeholder="e.g. Reading"
                    value={draft.newCategory ?? ""}
                    onChange={(e) => change("newCategory", e.target.value)}
                  />
                  <IconPicker paged={floating}
                    value={draft.newCategoryIcon || "folder"}
                    onChange={(v) => change("newCategoryIcon", v)}
                  />
                </div>
              )}

              <label htmlFor="edit-title">{floating ? "Title" : "Title (optional)"}</label>
              <input
                ref={titleField}
                id="edit-title"
                required={floating}
                aria-invalid={titleError || undefined}
                aria-describedby={titleError ? "capture-title-error" : undefined}
                data-initial-focus={(floating && !!capturedUrl) || undefined}
                name="title"
                autoComplete="off"
                value={draft.title}
                onChange={(e) => change("title", e.target.value)}
              />

              {titleError && <p id="capture-title-error" className="error title-error" role="alert">Enter a title to save this link.</p>}

              {floating ? (
                <details
                  ref={detailsRef}
                  className="capture-details"
                  onToggle={(e) => void resizeDetails(creatingCategory || e.currentTarget.open)}
                >
                  <summary>
                    <SymbolIcon name="settings" />
                    <span>More details</span>
                    <SymbolIcon name="chevron" className="details-chevron" />
                  </summary>
                  <div className="details-fields">{details}</div>
                </details>
              ) : (
                details
              )}
            </fieldset>
          </div>

          {error && (
            <p ref={errorRef} tabIndex={-1} role="alert" className="error">
              {error}
            </p>
          )}

          <div className="dialog-actions">
            <button
              type="button"
              className="secondary"
              disabled={pending || exiting}
              onClick={cancel}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={pending || exiting || !!committedMessage}
            >
              {pending ? "Saving…" : link ? "Save changes" : "Save link"}
            </button>
          </div>
          {committedMessage && (
            <p className="hint" role="status">
              {committedMessage}
            </p>
          )}
          {panelError && (
            <p role="alert" className="error">
              {panelError}
            </p>
          )}

          {!floating && (
            <p className="hint">
              Ctrl+Enter to save. Optional details can stay empty.
            </p>
          )}
        </form>
      </Modal>

      {discard && (
        <ConfirmDialog
          title="Discard unsaved changes?"
          confirmLabel="Discard changes"
          cancelLabel="Keep editing"
          onConfirm={onCancel}
          onCancel={() => setDiscard(false)}
        >
          <p>Your saved links will stay unchanged.</p>
        </ConfirmDialog>
      )}
    </>
  );
}
