import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CategoryGlyph } from "./CategoryEditor";

export function categorySymbol(name: string) {
  if (name.toLowerCase() === "inbox") return "inbox";
  if (/book|read|study|learn/i.test(name)) return "book";
  return "folder";
}

export function faviconUrl(value: string): string | undefined {
  const url = URL.parse(value);
  if (!url || !["http:", "https:"].includes(url.protocol)) return;
  // Omit private IP literals, credentials, ports and all saved path/query details.
  if (
    url.username ||
    url.password ||
    url.port ||
    !url.hostname.includes(".") ||
    /^[0-9.]+$/.test(url.hostname) ||
    url.hostname.includes(":") ||
    /(^|\.)(localhost|local|internal|home|test|invalid)$/.test(url.hostname)
  )
    return;
  return `https://${url.hostname}/favicon.ico`;
}

export function WebsiteIcon({
  id,
  url,
  category,
  icon,
}: {
  id: number;
  url: string;
  category: string;
  icon?: string;
}) {
  const element = useRef<HTMLSpanElement>(null);
  const [loaded, setLoaded] = useState<{ key: string; data: string }>();
  const src = faviconUrl(url);
  useEffect(() => {
    if (!src || !element.current) return;
    let active = true;
    let inView = typeof IntersectionObserver === "undefined";
    let queued = false;
    const ready = () =>
      active &&
      inView &&
      navigator.onLine &&
      !document.hidden &&
      document.hasFocus();
    const load = () => {
      if (!ready() || queued) return;
      queued = true;
      iconQueue = iconQueue.then(async () => {
        try {
          if (!ready()) return;
          if (!iconCache.has(src)) {
            const data = await invoke<string>("site_icon", { id });
            rememberIcon(
              src,
              typeof data === "string" &&
                /^data:image\/(?:png|jpeg|x-icon);base64,/.test(data)
                ? data
                : undefined,
            );
          }
          const data = iconCache.get(src);
          if (active && data) setLoaded({ key: src, data });
        } catch {
          rememberIcon(src, undefined);
        } finally {
          queued = false;
        }
      });
    };
    const reconnect = () => {
      if (iconCache.get(src) === undefined) iconCache.delete(src);
      load();
    };
    const observer =
      typeof IntersectionObserver === "undefined"
        ? undefined
        : new IntersectionObserver(([entry]) => {
            inView = entry.isIntersecting;
            load();
          });
    observer?.observe(element.current);
    window.addEventListener("online", reconnect);
    window.addEventListener("focus", load);
    document.addEventListener("visibilitychange", load);
    const cached = iconCache.get(src);
    if (cached) setLoaded({ key: src, data: cached });
    else load();
    return () => {
      active = false;
      observer?.disconnect();
      window.removeEventListener("online", reconnect);
      window.removeEventListener("focus", load);
      document.removeEventListener("visibilitychange", load);
    };
  }, [id, src]);
  return (
    <span className="website-icon" ref={element} aria-hidden="true">
      {loaded && loaded.key === src ? (
        <img
          src={loaded.data}
          alt=""
          loading="lazy"
          decoding="async"
          onError={() => {
            rememberIcon(loaded.key, undefined);
            setLoaded(undefined);
          }}
        />
      ) : (
        <CategoryGlyph icon={icon || categorySymbol(category)} />
      )}
    </span>
  );
}

const iconCache = new Map<string, string | undefined>();
let iconQueue = Promise.resolve();

function rememberIcon(key: string, image: string | undefined) {
  iconCache.set(key, image);
  const oldest = iconCache.keys().next();
  if (iconCache.size > 32 && !oldest.done) iconCache.delete(oldest.value);
}
