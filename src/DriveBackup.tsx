import { useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Modal } from "./Modal";
import { SymbolIcon } from "./GlassDetails";
import { errorMessage } from "./links";

type Frequency = "off" | "daily" | "weekly" | "monthly";

export interface DriveStatus {
  configured: boolean;
  requiresReconnect: boolean;
  profile: { name: string; photo?: string | null } | null;
  frequency: Frequency;
  lastBackup: number | null;
  warning: string | null;
}

type Action =
  | { kind: "status" | "connect" | "disconnect" | "backup" }
  | { kind: "schedule"; frequency: Frequency };

export function driveBackup(action: Action = { kind: "status" }) {
  return invoke<DriveStatus>("drive_backup", { action });
}

type Profile = NonNullable<DriveStatus["profile"]>;
type BackupEntry = {
  id: string; name: string; createdTime: string; size: number;
  kind?: "Manual" | "Automatic" | null; linkCount?: number | null;
};

const backupDate = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
function backupLabel(entry: BackupEntry) {
  const date = new Date(entry.createdTime);
  const when = Number.isNaN(date.getTime()) ? entry.name || "Undated backup" : backupDate.format(date);
  const count = entry.linkCount;
  return [when, entry.kind ?? "Earlier backup", `${Math.ceil(entry.size / 1024)} KB`,
    count == null ? null : `${count} ${count === 1 ? "link" : "links"}`].filter(Boolean).join(" · ");
}

export function DriveAccount({ profile }: { profile: Profile }) {
  const initials = Array.from(profile.name.trim())[0]?.toLocaleUpperCase() || "G";
  return <span className="drive-account">
    <span className="drive-avatar" aria-hidden="true">
      {profile.photo ? <img src={profile.photo} width={30} height={30} alt="" /> : initials}
    </span>
    <span className="drive-identity">
      <strong>{profile.name}</strong>
      <small>Google Drive backups</small>
    </span>
  </span>;
}

function RestoreBackup({ onClose, onRestored }: { onClose: () => void; onRestored: () => void }) {
  const descriptionId = useId();
  const [entries, setEntries] = useState<BackupEntry[]>();
  const [id, setId] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");

  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
    void invoke<BackupEntry[]>("list_drive_backups").then(
      backups => { if (current) setEntries(backups); },
      cause => { if (current) setError(errorMessage(cause)); },
    );
    return () => { current = false; };
  }, [attempt]);

  async function restore() {
    if (!id || working) return;
    setWorking(true);
    setError("");
    try {
      await invoke<void>("restore_drive_backup", { id });
      onRestored();
    }
    catch (cause) { setError(errorMessage(cause)); setWorking(false); }
  }

  const cancel = () => { if (!working) onClose(); };
  return <Modal title="Restore a Drive backup" describedBy={descriptionId} onCancel={cancel}>
    <p className="hint">Choose a snapshot from your Google Drive backup history.</p>
    {entries?.length ? <>
      <label htmlFor="drive-restore-file">Backup to restore</label>
      <select id="drive-restore-file" value={id} disabled={working} onChange={event => setId(event.target.value)}>
        <option value="">Choose a backup</option>
        {entries.map(entry => <option key={entry.id} value={entry.id}>
          {backupLabel(entry)}
        </option>)}
      </select>
    </> : !error && <p role="status">
      {entries ? "No Linkme backups were found in this Google Drive." : "Finding backups…"}
    </p>}
    <p id={descriptionId} className="restore-impact">Restoring replaces the links, categories and groups on this device. Linkme first saves a local recovery copy. Your settings and Google account stay connected.</p>
    {error && <p role="alert" className="error">{error}</p>}
    <span role="status" className="sr-only">{working ? "Restoring backup…" : ""}</span>
    <div className="dialog-actions">
      <button type="button" className="secondary" data-initial-focus aria-disabled={working || undefined} onClick={cancel}>Cancel</button>
      {entries?.length ? <button type="button" className="danger" disabled={!id} aria-disabled={working || undefined}
        onClick={() => void restore()}>{working ? "Restoring…" : "Replace library"}</button>
      : (entries || error) && <button type="button" onClick={() => { setEntries(undefined); setError(""); setAttempt(n => n + 1); }}>Refresh list</button>}
    </div>
  </Modal>;
}

export function DriveSettings({ status, onChange, onRestored }: {
  status?: DriveStatus; onChange: (status: DriveStatus) => void; onRestored?: () => void;
}) {
  const [working, setWorking] = useState<Action["kind"]>();
  const [choosingBackup, setChoosingBackup] = useState(false);
  const [error, setError] = useState("");
  const [privacyError, setPrivacyError] = useState("");
  const [notice, setNotice] = useState("");
  async function openPrivacyPolicy() {
    setPrivacyError("");
    try {
      await invoke("open_drive_privacy_policy");
    } catch (cause) {
      setPrivacyError(errorMessage(cause));
    }
  }
  async function run(action: Action) {
    setWorking(action.kind);
    setError("");
    setNotice("");
    try {
      onChange(await driveBackup(action));
      if (action.kind === "backup") setNotice("Backup saved to Google Drive.");
      if (action.kind === "disconnect") setNotice("Disconnected. Existing Drive backups remain.");
    }
    catch (cause) { setError(errorMessage(cause)); }
    finally { setWorking(undefined); }
  }
  return <><fieldset className="drive-settings" disabled={!!working}>
    <legend>Cloud backup</legend>
    {status?.profile ? <>
      <div className="drive-summary">
        <DriveAccount profile={status.profile} />
        <span className="hint">{status.requiresReconnect ? "Reconnect to resume"
          : status.lastBackup ? `Last backup: ${backupDate.format(status.lastBackup * 1000)}` : "Connected · No backup yet"}</span>
      </div>
      <div className="drive-actions">
        <button type="button" disabled={!status.configured || status.requiresReconnect} onClick={() => void run({kind:"backup"})}>
          {working === "backup" ? "Backing up…" : "Back up now"}
        </button>
        <button type="button" className="secondary compact" disabled={!status.configured || status.requiresReconnect} onClick={() => setChoosingBackup(true)}>
          <SymbolIcon name="inbox" /> Backup history…
        </button>
      </div>
      <div className="drive-schedule">
        <label htmlFor="backup-frequency">Automatic backups</label>
        <select id="backup-frequency" value={status.frequency} disabled={!status.configured || status.requiresReconnect}
          onChange={event => void run({kind:"schedule", frequency:event.target.value as Frequency})}>
          <option value="off">Off · keep 7 manual backups</option>
          <option value="daily">Daily · keep 7</option>
          <option value="weekly">Weekly · keep 4</option>
          <option value="monthly">Monthly · keep 12</option>
        </select>
        <p className="hint">Linkme saves snapshots of this device's library. Older copies from this installation move to Drive Trash after a successful backup.</p>
      </div>
      <div className="drive-actions">
        <button type="button" className="secondary compact" disabled={!status.configured} onClick={() => void run({kind:"connect"})}>
          {working === "connect" ? "Waiting for Google…" : "Reconnect"}
        </button>
        <button type="button" className="secondary compact" onClick={() => void run({kind:"disconnect"})}>Disconnect</button>
      </div>
    </> : <>
      <p className="hint">Back up your Linkme library to your own Drive. Connecting opens Google sign-in in your browser.</p>
      {!status?.configured && <p className="hint">Google sign-in needs Linkme’s Desktop OAuth configuration on this device.</p>}
      <button type="button" className="secondary compact" disabled={!status?.configured} onClick={() => void run({kind:"connect"})}>
        {working === "connect" ? "Waiting for Google…" : "Connect Google Drive"}
      </button>
    </>}
    <p className="hint drive-notice" role="status">{working === "backup" ? "Saving a snapshot to Google Drive…" : notice}</p>
    {(error || status?.warning) && <p className="error" role="alert">{error || status?.warning}</p>}
    <p className="hint">
      <a
        href="https://linkme.bacontrack.tn/privacy"
        onClick={(event) => {
          event.preventDefault();
          void openPrivacyPolicy();
        }}
      >
        Linkme privacy policy
      </a>{" "}
      (opens in your browser)
    </p>
    {privacyError && <p className="error" role="alert">{privacyError}</p>}
  </fieldset>
  {choosingBackup && <RestoreBackup onClose={() => setChoosingBackup(false)} onRestored={() => {
    setChoosingBackup(false);
    onRestored?.();
  }} />}
  </>;
}
