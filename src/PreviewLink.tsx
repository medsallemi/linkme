import { invoke } from "@tauri-apps/api/core";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

interface PageMetadata {
  title: string;
  description: string;
  image?: string;
}

export function previewPreference(enabled?: boolean) {
  return invoke<boolean>("preview_preference", { enabled });
}

export function PreviewLink({
  id, url, label, enabled, disabled, onOpen, icon, displayUrl, subtitle,
}: {
  id: number;
  url: string;
  label: string;
  enabled: boolean;
  disabled: boolean;
  onOpen: () => void;
  icon?: ReactNode;
  displayUrl?: boolean;
  subtitle?: string;
}) {
  const descriptionId = useId();
  const anchor = useRef<HTMLDivElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const leave = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [active, setActive] = useState(false);
  const [target, setTarget] = useState("");
  const [result, setResult] = useState<PageMetadata | "unavailable" | null>(null);
  const address = URL.parse(url);
  const fileName = address?.pathname.split("/").pop() ?? "";
  const extension = /\.(pdf|docx?|xlsx?|pptx?|csv|txt|zip|png|jpe?g|webp|gif|svg|mp[34]|webm|mov)$/i.exec(fileName)?.[1];
  const fallback = extension ? `${extension.toUpperCase()} file · Saved link` : "Saved link · Website preview unavailable";
  const eligible = enabled && !disabled && !!address;
  const showing = eligible && active && target === url;

  useEffect(() => {
    if (!eligible || !active || !navigator.onLine || document.hidden) return;
    let current = true;
    const close = () => {
      setActive(false);
      setTarget("");
      setResult(null);
    };
    const supersede = (event: Event) => {
      if ((event as CustomEvent<string>).detail !== descriptionId) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    const delay = setTimeout(() => {
      setTarget(url);
      setResult(null);
      void invoke<PageMetadata>("preview_link", { id }).then(
        (data) => { if (current) setResult(data); },
        () => { if (current) setResult("unavailable"); },
      );
    }, 450);
    window.addEventListener("offline", close);
    window.addEventListener("blur", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    document.addEventListener("visibilitychange", close);
    document.addEventListener("keydown", escape);
    document.addEventListener("linkme-preview-start", supersede);
    return () => {
      current = false;
      clearTimeout(delay);
      clearTimeout(leave.current);
      window.removeEventListener("offline", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      document.removeEventListener("visibilitychange", close);
      document.removeEventListener("keydown", escape);
      document.removeEventListener("linkme-preview-start", supersede);
    };
  }, [active, descriptionId, eligible, id, url]);

  function arm() {
    clearTimeout(leave.current);
    if (!navigator.onLine) return;
    document.dispatchEvent(new CustomEvent("linkme-preview-start", { detail: descriptionId }));
    setActive(true);
  }

  function dismiss() {
    clearTimeout(leave.current);
    setActive(false);
    setTarget("");
    setResult(null);
  }

  function pointerLeave() {
    if (!showing) dismiss();
    else leave.current = setTimeout(dismiss, 180);
  }

  useLayoutEffect(() => {
    if (!showing || !anchor.current || !card.current) return;
    const rect = anchor.current.getBoundingClientRect();
    const { offsetWidth, offsetHeight } = card.current;
    const x = Math.max(8, Math.min(rect.left, window.innerWidth - offsetWidth - 8));
    const below = rect.bottom;
    const above = rect.top - offsetHeight;
    const y = below + offsetHeight <= window.innerHeight - 8 ? below : Math.max(8, above);
    card.current.style.left = `${x}px`;
    card.current.style.top = `${y}px`;
  }, [showing, result]);

  return (
    <div ref={anchor} className="link-content"
      onPointerEnter={eligible ? event => { if (event.pointerType !== "touch") arm(); } : undefined}
      onPointerLeave={pointerLeave}>
      {icon}
      <button
        className="link-title"
        title={eligible ? undefined : url}
        aria-label={`Open ${label}`}
        aria-describedby={showing ? descriptionId : undefined}
        disabled={disabled}
        onClick={() => { dismiss(); onOpen(); }}
        onFocus={eligible ? (event) => {
          if (event.currentTarget.matches(":focus-visible")) arm();
        } : undefined}
        onBlur={dismiss}
      >
        {label}
      </button>
      {displayUrl && <p className="link-url">{url}</p>}
      {subtitle && <p className="subtitle">{subtitle}</p>}
      {showing && createPortal(
        <div ref={card} id={descriptionId} className="page-preview" role="tooltip"
          onPointerEnter={() => clearTimeout(leave.current)}
          onPointerLeave={pointerLeave}>
          {result && result !== "unavailable" && result.image && (
            <img className="page-preview-image" src={result.image} alt="" decoding="async"
              onError={() => setResult({ ...result, image: undefined })} />
          )}
          <div className="page-preview-host">{address?.hostname}</div>
          {result === null ? (
            <div className="page-preview-loading" role="status">
              Loading preview<span aria-hidden="true" />
            </div>
          ) : result === "unavailable" ? (
            <><strong>{label}</strong>{subtitle && <p>{subtitle}</p>}<p>{fallback}</p></>
          ) : (
            <>
              <strong>{result.title || label}</strong>
              {result.description && <p>{result.description}</p>}
            </>
          )}
        </div>,
        document.body,
      )}
    </div>
  );
}
