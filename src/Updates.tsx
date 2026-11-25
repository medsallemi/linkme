import { invoke } from "@tauri-apps/api/core";
import { useEffect, useEffectEvent, useId, useRef, useState } from "react";
import { SymbolIcon } from "./GlassDetails";
import { Modal } from "./Modal";
import "./updates.css";

type Release = { version: string; publishedAt: string; notes: string };
type UpdateStatus = {
  installed: string;
  configured: boolean;
  automatic: boolean;
  lastChecked: number | null;
  release: Release | null;
};
type Check = {
  outcome: "available" | "current" | "failed" | "skipped" | "unconfigured";
  opening: number | null;
  status: UpdateStatus;
};

const unavailable = "Unable to check for updates. Please try again.";
const openError =
  "Could not open GitHub. Check your default browser and try again.";

export function useUpdateNotice(blocked: boolean) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const session = useRef({ generation: 0, busy: false, live: false });
  const canShow = useEffectEvent(
    () =>
      !blocked &&
      !status &&
      document.hasFocus() &&
      !document.hidden &&
      !document.querySelector("dialog[open]"),
  );
  const check = useEffectEvent(async () => {
    const state = session.current;
    if (
      !state.live ||
      state.busy ||
      status ||
      !document.hasFocus() ||
      document.hidden
    )
      return;
    const generation = state.generation;
    const current = () => state.live && state.generation === generation;
    state.busy = true;
    try {
      const result = await invoke<Check>("check_for_updates", {
        manual: false,
      });
      if (
        !current() ||
        !canShow() ||
        result.outcome !== "available" ||
        result.opening === null
      )
        return;
      const notice = await invoke<UpdateStatus | null>("claim_update_notice", {
        opening: result.opening,
      });
      if (notice?.release && current() && canShow()) setStatus(notice);
    } catch {
      // Automatic checks never put connection or IPC errors in front of the user.
    } finally {
      if (current()) state.busy = false;
    }
  });

  useEffect(() => {
    const state = session.current;
    state.live = true;
    const refresh = () => {
      void check();
    };
    const endOpening = () => {
      state.generation++;
      state.busy = false;
      setStatus(null);
    };
    const reopened = () => {
      endOpening();
      refresh();
    };
    window.addEventListener("linkme-library-open", reopened);
    window.addEventListener("linkme-library-minimized", endOpening);
    window.addEventListener("linkme-library-ready", refresh);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    refresh();
    return () => {
      state.live = false;
      state.generation++;
      state.busy = false;
      window.removeEventListener("linkme-library-open", reopened);
      window.removeEventListener("linkme-library-minimized", endOpening);
      window.removeEventListener("linkme-library-ready", refresh);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, []);
  useEffect(() => {
    if (blocked && status) setStatus(null);
    void check();
  }, [blocked, status]);

  // Give explicit Library workflows priority even if both states change in one render.
  const release = status?.release;
  return {
    notice:
      release && !blocked ? (
        <UpdateNotice
          installed={status.installed}
          release={release}
          onClose={() => setStatus(null)}
        />
      ) : null,
  };
}

function UpdateNotice({
  installed,
  release,
  onClose,
}: {
  installed: string;
  release: Release;
  onClose: () => void;
}) {
  const description = useId();
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  return (
    <Modal
      title="A better Linkme awaits."
      describedBy={description}
      onCancel={onClose}
      headingAdornment={
        <>
          <span className="update-mark" aria-hidden="true">
            <SymbolIcon name="update" />
          </span>
          <button
            className="secondary update-close"
            aria-label="Close update"
            onClick={onClose}
          >
            <SymbolIcon name="close" />
          </button>
        </>
      }
    >
      <div className="update-content">
        <p id={description}>
          Download the Windows installer from GitHub and run it when you’re
          ready.
        </p>
        <dl className="update-versions">
          <div>
            <dt>Installed</dt>
            <dd>{installed}</dd>
          </div>
          <div>
            <dt>Available</dt>
            <dd>{release.version}</dd>
          </div>
        </dl>
        <h3>What’s new</h3>
        <time dateTime={release.publishedAt}>
          {new Intl.DateTimeFormat(undefined, {
            dateStyle: "medium",
            timeZone: "UTC",
          }).format(new Date(release.publishedAt))}
        </time>
        <p className="update-notes">
          {release.notes || "See GitHub for release details."}
        </p>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button
            type="button"
            className="secondary"
            data-initial-focus
            onClick={onClose}
          >
            Remind me later
          </button>
          <button
            type="button"
            aria-disabled={opening || undefined}
            onClick={async () => {
              if (opening) return;
              setOpening(true);
              setError("");
              try {
                await invoke("open_update_release");
                onClose();
              } catch {
                setError(openError);
                setOpening(false);
              }
            }}
          >
            Update now
          </button>
        </div>
      </div>
    </Modal>
  );
}

export function UpdateSettings() {
  const [status, setStatus] = useState<UpdateStatus>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const help = useId();

  useEffect(() => {
    let live = true;
    void invoke<UpdateStatus>("update_preferences").then(
      (result) => {
        if (live) setStatus(result);
      },
      () => {
        if (live) setMessage("Could not load update settings.");
      },
    );
    return () => {
      live = false;
    };
  }, []);

  return (
    <section className="update-settings" aria-label="Updates">
      <h3>Updates</h3>
      {status && <p className="hint">Installed version {status.installed}</p>}
      <label className="preview-setting">
        <input
          type="checkbox"
          checked={status?.automatic ?? true}
          disabled={!status || busy}
          aria-describedby={help}
          onChange={async (event) => {
            const enabled = event.currentTarget.checked;
            setBusy(true);
            setMessage("");
            try {
              setStatus(
                await invoke<UpdateStatus>("update_preferences", { enabled }),
              );
            } catch {
              setMessage("Could not save the update preference.");
            } finally {
              setBusy(false);
            }
          }}
        />
        Automatic update checks
      </label>
      <p id={help} className="hint">
        When enabled, Linkme contacts GitHub after you open the Library.
      </p>
      {status && !status.configured && (
        <p className="hint">
          This build has no configured GitHub release source.
        </p>
      )}
      <div className="update-actions">
        <button
          type="button"
          className="secondary"
          aria-disabled={!status?.configured || busy}
          onClick={async () => {
            if (!status?.configured || busy) return;
            setBusy(true);
            setMessage("Checking for updates…");
            try {
              const result = await invoke<Check>("check_for_updates", {
                manual: true,
              });
              setStatus(result.status);
              setMessage(
                result.outcome === "available"
                  ? "A new version is available."
                  : result.outcome === "current"
                    ? "You're up to date."
                    : unavailable,
              );
            } catch {
              setMessage(unavailable);
            } finally {
              setBusy(false);
            }
          }}
        >
          Check for updates
        </button>
        {status?.release && (
          <button
            type="button"
            className="secondary"
            onClick={async () => {
              try {
                await invoke("open_update_release");
              } catch {
                setMessage(openError);
              }
            }}
          >
            View release notes
          </button>
        )}
      </div>
      {status && (
        <p className="hint">
          Last successful check{" "}
          {status.lastChecked === null
            ? "—"
            : new Date(status.lastChecked * 1000).toLocaleString()}
        </p>
      )}
      {status?.release && <p>Version {status.release.version} available</p>}
      <p role="status" className="hint update-status">
        {message}
      </p>
    </section>
  );
}
