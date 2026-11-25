import { useEffect, useEffectEvent, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage, listLinks } from "./links";
import type { Link, LinkPage } from "./links";
import { WebsiteIcon } from "./WebsiteIcon";
import { CategoryGlyph } from "./CategoryEditor";
import { SymbolIcon } from "./GlassDetails";
import { usePopupDismiss } from "./popupMotion";
import { usePopupReady } from "./popupReady";

type Results = { query: string; page?: LinkPage; error?: string };

export function SearchOverlay() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Results>();
  const [selection, setSelection] = useState(0);
  const [actionError, setActionError] = useState("");
  const [notice, setNotice] = useState("");
  const opening = useRef(false);
  const grid = useRef<HTMLDivElement>(null);
  const [pageSize, setPageSize] = useState(3);
  const input = useRef<HTMLInputElement>(null);
  const { dismiss, exiting } = usePopupDismiss(
    () => invoke("dismiss_capture_panel"),
    (error) => setActionError(errorMessage(error)),
  );
  const current = results?.query === query ? results : undefined;
  const links = current?.page?.links ?? [];
  const pageStart = Math.floor(selection / pageSize) * pageSize;
  const visibleLinks = links.slice(pageStart, pageStart + pageSize);
  const active = links[selection];
  const resultCount = current?.page?.links.length;
  const usedSearch = useRef(false);
  useEffect(() => {
    if (query) usedSearch.current = true;
    if (usedSearch.current && resultCount !== undefined) void invoke("resize_search_panel", { results: resultCount }).catch(() => {});
  }, [query, resultCount]);
  const activeId = active && `search-result-${active.id}`;

  useEffect(() => {
    input.current?.focus();
  }, []);
  usePopupReady(true);
  useEffect(() => {
    let valid = true;
    const timer = setTimeout(
      () => {
        void listLinks(query).then(
          (page) => {
            if (valid) setResults({ query, page });
          },
          (error) => {
            if (valid) setResults({ query, error: errorMessage(error) });
          },
        );
      },
      query === "" ? 0 : 140,
    );
    return () => {
      valid = false;
      clearTimeout(timer);
    };
  }, [query]);
  useEffect(() => {
    const element = grid.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      const rowHeight = parseFloat(getComputedStyle(element).getPropertyValue("--search-row-height"));
      if (rowHeight) setPageSize(Math.max(1, Math.floor(entry.contentRect.height / rowHeight)));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(""), 2400);
    return () => clearTimeout(timer);
  }, [notice]);

  function change(value: string) {
    setNotice("");
    setQuery(value);
    setSelection(0);
    setActionError("");
  }

  const escape = useEffectEvent((event: KeyboardEvent) => {
    if (
      event.key !== "Escape" ||
      event.isComposing ||
      opening.current ||
      exiting
    )
      return;
    event.preventDefault();
    if (query) change("");
    else dismiss();
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => escape(event);
    document.addEventListener("keydown", listener);
    return () => document.removeEventListener("keydown", listener);
  }, []);

  async function open(link: Link) {
    if (opening.current || exiting) return;
    opening.current = true;
    setActionError("");
    try {
      await invoke("open_search_result", { id: link.id });
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      opening.current = false;
    }
  }

  async function act(link: Link, action: "copy" | "share") {
    if (opening.current || exiting) return;
    opening.current = true;
    setActionError("");
    setNotice("");
    try {
      if (action === "copy") {
        await invoke("copy_link", { id: link.id });
        setNotice("Link copied.");
      } else if (typeof navigator.share === "function") {
        await navigator.share({
          title: link.title || undefined,
          url: link.url,
        });
      } else {
        await invoke("share_link_email", { id: link.id });
        setNotice("Email draft opened.");
      }
    } catch (err) {
      if (!(err instanceof DOMException && err.name === "AbortError"))
        setActionError(errorMessage(err));
    } finally {
      opening.current = false;
    }
  }
  return (
    <main className="search-overlay" data-exiting={exiting || undefined}>
      <header className="glass-heading">
        <label htmlFor="overlay-query">Search saved links</label>
        <button
          className="capture-close"
          type="button"
          aria-label="Close search"
          disabled={exiting}
          onClick={() => {
            if (!opening.current) dismiss();
          }}
        >
          <SymbolIcon name="close" />
        </button>
      </header>
      <input
        ref={input}
        id="overlay-query"
        role="combobox"
        aria-autocomplete="list"
        aria-haspopup="grid"
        aria-controls="overlay-results"
        aria-expanded={links.length > 0}
        aria-activedescendant={activeId}
        autoComplete="off"
        spellCheck={false}
        maxLength={256}
        placeholder="Title, URL, notes or category"
        value={query}
        readOnly={exiting}
        onChange={(event) => change(event.target.value)}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          switch (event.key) {
            case "ArrowDown":
            case "ArrowUp":
              if (!links.length) return;
              event.preventDefault();
              setSelection((index) =>
                Math.max(
                  0,
                  Math.min(
                    links.length - 1,
                    index + (event.key === "ArrowDown" ? 1 : -1),
                  ),
                ),
              );
              break;
            case "Enter":
              event.preventDefault();
              if (active) void open(active);
              break;
          }
        }}
      />
      {(current?.error || actionError) && (
        <p role="alert" className="error">
          {actionError || current?.error}
        </p>
      )}
      <p className="hint search-overlay-status" role="status">
        {notice ||
          (!current
            ? "Searching…"
            : current.error
              ? "Close search and retry to reload."
              : links.length === 0
                ? query
                  ? "No matching links."
                  : "No saved links yet."
                : current.page?.next
                  ? "First 100 matches. Refine your search for more."
                  : `${links.length} ${links.length === 1 ? "result" : "results"}`)}
      </p>
      <div
        ref={grid}
        id="overlay-results"
        className="search-overlay-results"
        role="grid"
        aria-label="Saved link results"
        aria-rowcount={links.length}
      >
        {visibleLinks.map((link, offset) => {
          const index = pageStart + offset;
          return (
          <div
            role="row"
            key={link.id}
            id={`search-result-${link.id}`}
            aria-rowindex={index + 1}
            aria-selected={index === selection}
            data-capture-highlight={link.categoryColor ?? undefined}
            onFocus={() => setSelection(index)}
          >
            <div role="gridcell" className="result-main">
              <button
                className="result-link"
                tabIndex={index === selection ? 0 : -1}
                aria-label={"Open " + (link.title || link.url)}
                aria-describedby={`search-result-${link.id}-context`}
                onClick={() => {
                  setSelection(index);
                  void open(link);
                }}
              >
                <WebsiteIcon
                  id={link.id}
                  url={link.url}
                  category={link.categoryName}
                  icon={link.categoryIcon}
                />
                <strong>{link.title || link.url}</strong>
                <span className="result-meta" id={`search-result-${link.id}-context`}>
                  <span className="category-tag">
                    <CategoryGlyph icon={link.categoryIcon || "folder"} />
                    {link.categoryName}
                  </span>
                  {link.groupName && <span className="category-tag"><SymbolIcon name="folder" />{link.groupName}</span>}
                  {link.title && <small>{link.url}</small>}
                </span>
              </button>
            </div>
            <div role="gridcell" className="result-tools">
              <button
                className="icon-action"
                tabIndex={index === selection ? 0 : -1}
                title="Copy URL"
                aria-label={"Copy URL for " + (link.title || link.url)}
                onClick={() => void act(link, "copy")}
              >
                <SymbolIcon name="copy" />
              </button>
              <button
                className="icon-action"
                tabIndex={index === selection ? 0 : -1}
                title={
                  typeof navigator.share === "function"
                    ? "Share link"
                    : "Share by email"
                }
                aria-label={"Share " + (link.title || link.url)}
                onClick={() => void act(link, "share")}
              >
                <SymbolIcon name="share" />
              </button>
            </div>
          </div>
          );
        })}
      </div>
      <div className="popup-pages search-pages" aria-label="Search results pages">
        {links.length > pageSize && <>
          <button type="button" className="secondary compact" aria-label="Previous results" aria-disabled={pageStart === 0}
            onClick={() => setSelection(Math.max(0, pageStart - pageSize))}>Previous</button>
          <span>{pageStart + 1}–{pageStart + visibleLinks.length} of {links.length}</span>
          <button type="button" className="secondary compact" aria-label="Next results" aria-disabled={pageStart + pageSize >= links.length}
            onClick={() => { if (pageStart + pageSize < links.length) setSelection(pageStart + pageSize); }}>Next</button>
        </>}
      </div>
      <p className="hint search-overlay-keys">
        ↑ ↓ Select · Enter Open · Esc {query ? "Clear" : "Close"}
      </p>
    </main>
  );
}
