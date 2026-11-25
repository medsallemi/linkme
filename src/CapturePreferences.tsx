import { DriveSettings, type DriveStatus } from "./DriveBackup";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { resetLinkme, updateCapture } from "./capture";
import type { CaptureConnection, CaptureSettings } from "./capture";
import { errorMessage } from "./links";
import { ConfirmDialog, Modal } from "./Modal";

import { previewPreference } from "./PreviewLink";
import { highlightColors } from "./highlightColors";
import { UpdateSettings } from "./Updates";

export function CapturePreferences({
  settings,
  driveStatus,
  onDriveChanged,
  onRestored,
  previewsEnabled,
  onPreviewsChanged,
  onSaved,
  onAppearanceSaved,
  onClose,
}: {
  settings: CaptureSettings;
  driveStatus?: DriveStatus;
  onDriveChanged: (status: DriveStatus) => void;
  onRestored: () => void;
  previewsEnabled: boolean;
  onPreviewsChanged: (enabled: boolean) => void;
  onSaved: (connection: CaptureConnection) => void;
  onAppearanceSaved: (connection: CaptureConnection) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [savingAppearance, setSavingAppearance] = useState(false);
  const savedSettings = useRef(settings);
  const desiredAppearance = useRef({
    highlight: settings.highlight, dimPercent: settings.dimPercent, panelTransparency: settings.panelTransparency,
  });
  const appearanceQueue = useRef<Pick<CaptureSettings, "highlight" | "dimPercent" | "panelTransparency"> | null>(null);
  const writingAppearance = useRef(false);

  async function applyAppearance(change: Partial<Pick<CaptureSettings, "highlight" | "dimPercent" | "panelTransparency">>) {
    setDraft(current => ({ ...current, ...change }));
    const next = { ...desiredAppearance.current, ...change };
    desiredAppearance.current = next;
    appearanceQueue.current = next;
    document.documentElement.dataset.captureHighlight = String(next.highlight);
    if (writingAppearance.current) return;
    writingAppearance.current = true;
    setSavingAppearance(true);
    setError("");
    let applied: CaptureConnection | undefined;
    try {
      while (appearanceQueue.current) {
        const desired = appearanceQueue.current;
        appearanceQueue.current = null;
        const baseline = savedSettings.current;
        if (desired.highlight === baseline.highlight && desired.dimPercent === baseline.dimPercent
          && desired.panelTransparency === baseline.panelTransparency) continue;
        applied = await updateCapture({ ...baseline, ...desired });
        savedSettings.current = applied.settings;
      }
    } catch (cause) {
      appearanceQueue.current = null;
      setError(errorMessage(cause));
      const { highlight, dimPercent, panelTransparency } = savedSettings.current;
      desiredAppearance.current = { highlight, dimPercent, panelTransparency };
      setDraft(current => ({ ...current, ...desiredAppearance.current }));
      document.documentElement.dataset.captureHighlight = String(savedSettings.current.highlight);
    } finally {
      if (applied) onAppearanceSaved(applied);
      writingAppearance.current = false;
      setSavingAppearance(false);
    }
  }

  const [previewPending, setPreviewPending] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const [startup, setStartup] = useState<boolean>();
  const [startupError, setStartupError] = useState("");
  const [startupPending, setStartupPending] = useState(false);

  useEffect(() => {
    let live = true;
    void invoke<boolean>("startup_preference").then(
      enabled => { if (live) setStartup(enabled); },
      cause => { if (live) setStartupError(errorMessage(cause)); },
    );
    return () => { live = false; };
  }, []);

  const transparency = draft.panelTransparency * 2.8;
  return (
    <Modal
      title="Capture settings"
      onCancel={() => {
        if (!pending) onClose();
      }}
    >
      <form
        onSubmit={async (event) => {
          event.preventDefault();
          if (pending || savingAppearance) return;
          setPending(true);
          setError("");
          try {
            onSaved(await updateCapture(draft));
          } catch (error) {
            setError(errorMessage(error));
            setPending(false);
          }
        }}
      >
        <fieldset disabled={pending}>
          <label htmlFor="capture-mode">Capture mode</label>
          <select
            id="capture-mode"
            data-initial-focus
            value={draft.mode}
            onChange={(e) =>
              setDraft({
                ...draft,
                mode: e.target.value === "quick" ? "quick" : "prompt",
              })
            }
          >
            <option value="prompt">Prompt before saving</option>
            <option value="quick">Quick-save to Inbox</option>
          </select>
          <label htmlFor="capture-shortcut">Capture shortcut</label>
          <input
            id="capture-shortcut"
            aria-describedby="shortcut-help"
            value={draft.shortcut}
            autoComplete="off"
            spellCheck={false}
            maxLength={32}
            onChange={(e) => setDraft({ ...draft, shortcut: e.target.value })}
          />
          <label htmlFor="search-shortcut">Search shortcut</label>
          <input
            id="search-shortcut"
            aria-describedby="shortcut-help"
            value={draft.searchShortcut}
            autoComplete="off"
            spellCheck={false}
            maxLength={32}
            onChange={(e) =>
              setDraft({ ...draft, searchShortcut: e.target.value })
            }
          />
          <p className="hint" id="shortcut-help">
            Use Ctrl+Shift and a letter or digit. The shortcuts work while
            Linkme runs in the tray.
          </p>
          <label htmlFor="capture-dimming">Popup background dimming</label>
          <div className="setting-range">
            <input
              id="capture-dimming"
              type="range"
              min={0}
              max={40}
              step={1}
              value={draft.dimPercent}
              aria-valuetext={
                draft.dimPercent === 0 ? "Off" : `${draft.dimPercent}%`
              }
              aria-describedby="capture-dimming-help"
              onChange={(e) =>
                setDraft({
                  ...draft,
                  dimPercent: e.currentTarget.valueAsNumber,
                })
              }
            />
            <span aria-hidden="true">
              {draft.dimPercent === 0 ? "Off" : `${draft.dimPercent}%`}
            </span>
          </div>
          <p className="hint" id="capture-dimming-help">
            Dims the display while Capture or Search has focus. Choose 0% to
            turn it off.
          </p>
          <label htmlFor="capture-panel-transparency">
            Floating panel transparency
          </label>
          <div className="setting-range">
            <input
              id="capture-panel-transparency"
              type="range"
              min={0}
              max={70}
              step={2.8}
              value={transparency}
              aria-valuetext={
                draft.panelTransparency === 0
                  ? "Solid"
                  : `${Math.round(transparency)}%`
              }
              aria-describedby="capture-panel-transparency-help"
              onChange={(e) =>
                setDraft({
                  ...draft,
                  panelTransparency: Math.round(
                    e.currentTarget.valueAsNumber / 2.8,
                  ),
                })
              }
            />
            <span aria-hidden="true">
              {draft.panelTransparency === 0
                ? "Solid"
                : `${Math.round(transparency)}%`}
            </span>
          </div>
          <p className="hint" id="capture-panel-transparency-help">
            Adjusts Capture, Search and the save confirmation. Windows
            accessibility settings take priority.
          </p>
          <button
            type="button"
            className="secondary compact"
            onClick={() => void applyAppearance({ highlight: 7, dimPercent: 40, panelTransparency: 25 })}
          >
            Restore Linkme appearance
          </button>
          <fieldset className="color-options">
            <legend>Highlight color</legend>
            {highlightColors.map((color, index) => (
              <label
                className="color-choice"
                data-capture-highlight={index}
                key={color}
              >
                <input
                  type="radio"
                  name="highlight"
                  value={index}
                  checked={draft.highlight === index}
                  onChange={() => void applyAppearance({ highlight: index })}
                />
                <span className="color-dot" aria-hidden="true" />
                <span>{color}</span>
              </label>
            ))}
          </fieldset>
        </fieldset>
        <label className="preview-setting">
          <input type="checkbox" checked={previewsEnabled} disabled={pending || previewPending}
            aria-describedby="preview-help" onChange={async (event) => {
              const enabled = event.currentTarget.checked;
              setPreviewPending(true);
              setError("");
              try { onPreviewsChanged(await previewPreference(enabled)); }
              catch (cause) { setError(errorMessage(cause)); }
              finally { setPreviewPending(false); }
            }} />
          Link previews
        </label>
        <p className="hint" id="preview-help">
          Hover or focus a link to preview it online. Linkme requests public page details using the address, including its query parameters, and may contact a thumbnail host.
          This setting takes effect immediately.
        </p>
        <label className="preview-setting">
          <input type="checkbox" checked={startup ?? false} disabled={startup === undefined || startupPending || pending}
            aria-describedby="startup-help" onChange={async event => {
              const enabled = event.currentTarget.checked;
              setStartupPending(true);
              setStartupError("");
              try { setStartup(await invoke<boolean>("startup_preference", { enabled })); }
              catch (cause) { setStartupError(errorMessage(cause)); }
              finally { setStartupPending(false); }
            }} />
          Start Linkme when I sign in to Windows
        </label>
        <p className="hint" id="startup-help">Linkme opens in the tray for this Windows user. This setting takes effect immediately.</p>
        {startupError && <p className="error" role="status">{startupError}</p>}
        <UpdateSettings />
        <DriveSettings status={driveStatus} onChange={onDriveChanged} onRestored={onRestored} />
        <section className="reset-section" aria-label="Local data">
          <button type="button" className="danger" disabled={pending}
            onClick={() => { setError(""); setConfirmReset(true); }}>Reset Linkme…</button>
        </section>
        {error && !confirmReset && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button
            type="button"
            className="secondary"
            onClick={onClose}
            disabled={pending}
          >
            Cancel
          </button>
          <button type="submit" disabled={pending || savingAppearance}>
            {pending ? "Saving…" : "Save settings"}
          </button>
        </div>
      </form>
      {confirmReset && (
        <ConfirmDialog title="Reset Linkme?" confirmLabel="Erase data and restart"
          pending={pending} error={error}
          onCancel={() => { setConfirmReset(false); setError(""); }}
          onConfirm={async () => {
            if (pending) return;
            setPending(true);
            setError("");
            try { await resetLinkme(); }
            catch (cause) { setError(errorMessage(cause)); setPending(false); }
          }}>
          <p>Reset erases the current library, capture, preview and update preferences, and local migration backups. Linkme restarts with an empty Inbox. Open drafts are discarded.</p>
          <p>Your Google connection, backup schedule, Drive backups, local restore recovery copies, and Windows startup setting are kept.</p>
        </ConfirmDialog>
      )}
    </Modal>
  );
}
