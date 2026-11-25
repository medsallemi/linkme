import { DriveAccount, driveBackup, type DriveStatus } from "./DriveBackup";
import { LibraryTree } from "./LibraryTree";
import {
  useCallback,
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { connectCapture } from "./capture";
import type { CaptureConnection, CaptureEvent } from "./capture";
import { PreviewLink, previewPreference } from "./PreviewLink";
import { CapturePreferences } from "./CapturePreferences";

import type { MouseEvent } from "react";

import {
  copyLink,
  deleteLink,
  errorMessage,
  listCategories,
  listLinks,
  openLink,
} from "./links";

import type { Category, Cursor, Link, SaveResult } from "./links";

import { CategoryEditor } from "./CategoryEditor";
import { WebsiteIcon } from "./WebsiteIcon";
import { SymbolIcon } from "./GlassDetails";
import { LinkEditor } from "./LinkEditor";

import { ConfirmDialog } from "./Modal";
import { useUpdateNotice } from "./Updates";

function mergeLinks(current: Link[], incoming: Link[]): Link[] {
  return [...new Map([...current, ...incoming].map((l) => [l.id, l])).values()];
}

function closeActions(event: MouseEvent<HTMLButtonElement>) {
  const details = event.currentTarget.closest("details");
  if (details) {
    details.open = false;
    details.querySelector("summary")?.focus();
  }
}

function LinkRow({
  link,
  categoryIcon,
  nativeBusy,
  handle,
  rowProps,
  previewsEnabled,
  onEdit,
  onDelete,
  onNative,
}: {
  link: Link;
  categoryIcon?: string;
  nativeBusy: boolean;
  handle?: React.ReactNode;
  rowProps?: React.HTMLAttributes<HTMLLIElement>;
  previewsEnabled: boolean;
  onEdit: (link: Link, category?: boolean) => void;
  onDelete: (link: Link) => void;
  onNative: (action: "copy" | "open", id: number) => void;
}) {
  const label = link.title || link.url;

  return (
    <li className="link-row" data-link-id={link.id} {...rowProps}>{handle}
      <PreviewLink
        id={link.id} url={link.url} label={label}
        enabled={previewsEnabled} disabled={nativeBusy}
        onOpen={() => onNative("open", link.id)}
        icon={<WebsiteIcon id={link.id} icon={categoryIcon} url={link.url} category={link.categoryName} />}
        displayUrl={!!link.title} subtitle={link.subtitle}
      />

      <div className="row-tools">
        <button
          className="secondary compact"
          aria-label={`Copy URL for ${label}`}
          disabled={nativeBusy}
          onClick={() => onNative("copy", link.id)}
        >
          <SymbolIcon name="copy" /> Copy
        </button>

        <details
          className="row-actions"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.currentTarget.open = false;
              e.currentTarget.querySelector("summary")?.focus();
            }
          }}
        >
          <summary aria-label={`Actions for ${label}`} title="More actions">
            <SymbolIcon name="more" />
          </summary>
          <div className="action-menu">
            <button
              title="Edit"
              onClick={(e) => {
                closeActions(e);
                onEdit(link);
              }}
            >
              <SymbolIcon name="edit" />
              <span className="sr-only">Edit</span>
            </button>

            <button
              title="Change category"
              onClick={(e) => {
                closeActions(e);
                onEdit(link, true);
              }}
            >
              <SymbolIcon name="move" />
              <span className="sr-only">Change category</span>
            </button>

            <button
              title="Delete"
              onClick={(e) => {
                closeActions(e);
                onDelete(link);
              }}
            >
              <SymbolIcon name="trash" />
              <span className="sr-only">Delete</span>
            </button>
          </div>
        </details>
      </div>
    </li>
  );
}

export default function App() {
  const [editingCategory, setEditingCategory] = useState<Category | null>(null);
  const library = useRef<HTMLElement>(null);
  useEffect(() => {
    let entrance: Animation | undefined;
    function open() {
      if (
        !window.matchMedia?.("(prefers-reduced-motion: no-preference)").matches
      )
        return;
      entrance?.cancel();
      entrance = library.current?.animate?.(
        [
          { opacity: 0.35, transform: "translateY(10px) scale(.975)" },
          { opacity: 1, transform: "none" },
        ],
        { duration: 270, easing: "cubic-bezier(.22,.74,.23,1)" },
      );
    }
    open();
    window.addEventListener("linkme-library-open", open);
    return () => {
      window.removeEventListener("linkme-library-open", open);
      entrance?.cancel();
    };
  }, []);
  const [query, setQuery] = useState("");
  const [links, setLinks] = useState<Link[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);

  const [next, setNext] = useState<Cursor | null>(null);
  const [loading, setLoading] = useState(true);
  const [categoryLoading, setCategoryLoading] = useState(true);

  const [pageError, setPageError] = useState("");
  const [categoryError, setCategoryError] = useState("");
  const [actionError, setActionError] = useState("");
  const [status, setStatus] = useState("");
  useEffect(() => {
    if (!status) return;
    const clear = () => setStatus("");
    const timer = setTimeout(clear, 3200);
    window.addEventListener("linkme-library-open", clear);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("linkme-library-open", clear);
    };
  }, [status]);

  const [editor, setEditor] = useState<{
    link: Link | null;
    category: boolean;
  } | null>(null);
  const [revealed, setRevealed] = useState<Link | null>(null);
  const [capture, setCapture] = useState<CaptureConnection | null>(null);
  const [captureSetupError, setCaptureSetupError] = useState("");
  const [captureError, setCaptureError] = useState("");
  const [previewsEnabled, setPreviewsEnabled] = useState(false);
  useEffect(() => {
    let mounted = true;
    void previewPreference().then((enabled) => {
      if (mounted) setPreviewsEnabled(enabled);
    }).catch(() => {});
    return () => { mounted = false; };
  }, []);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [drive, setDrive] = useState<DriveStatus>();
  useEffect(() => {
    let live = true;
    const refresh = () => {
      void driveBackup().then(status => { if (live) setDrive(status); }).catch(() => {});
    };
    refresh();
    return () => { live = false; };
  }, [preferencesOpen]);

  const [deleting, setDeleting] = useState<Link | null>(null);
  const [deletePending, setDeletePending] = useState(false);
  const [deleteError, setDeleteError] = useState("");

  const [nativeBusy, setNativeBusy] = useState(false);
  const { notice: updateNotice } = useUpdateNotice(
    !!(editingCategory || editor || preferencesOpen || deleting || nativeBusy),
  );
  const nativeRef = useRef(false);
  const deleteRef = useRef(false);
  const request = useRef(0);
  const lastQuery = useRef("");
  const search = useRef<HTMLInputElement>(null);
  const afterDelete = useRef<{ anchor: HTMLElement | null; top: number } | null>(null);
  useLayoutEffect(() => {
    if (deleting || !afterDelete.current) return;
    const { anchor, top } = afterDelete.current;
    afterDelete.current = null;
    if (anchor?.isConnected) {
      const offset = anchor.getBoundingClientRect().top - top;
      if (offset && document.scrollingElement) document.scrollingElement.scrollTop += offset;
      const focus = anchor.matches("button")
        ? anchor
        : anchor.querySelector<HTMLElement>(".link-title");
      focus?.focus({ preventScroll: true });
    } else {
      search.current?.focus({ preventScroll: true });
    }
  }, [deleting]);

  const load = useCallback(
    async (value: string, before: Cursor | null = null) => {
      const current = ++request.current;
      setLoading(true);
      setPageError("");

      if (!before && lastQuery.current !== value) {
        setLinks([]);
        setNext(null);
      }
      lastQuery.current = value;

      try {
        const page = await listLinks(value, before);
        if (current !== request.current) return;
        setLinks((old) => (before ? mergeLinks(old, page.links) : page.links));
        setNext(page.next);
      } catch (error: unknown) {
        if (current === request.current)
          setPageError("Could not load links. " + errorMessage(error));
      } finally {
        if (current === request.current) setLoading(false);
      }
    },
    [],
  );

  const loadCategories = useCallback(async () => {
    setCategoryLoading(true);
    setCategoryError("");
    try {
      setCategories(await listCategories());
    } catch (error: unknown) {
      setCategoryError("Could not load categories. " + errorMessage(error));
    } finally {
      setCategoryLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadCategories();
  }, [loadCategories]);

  useEffect(() => {
    const timer = setTimeout(() => {
      void load(query);
    }, 150);
    return () => {
      clearTimeout(timer);
      request.current++;
    };
  }, [query, load]);

  function reload() {
    setActionError("");
    void load(query);
    void loadCategories();
  }

  function saved(result: SaveResult, edited: boolean) {
    setActionError("");
    setStatus(
      edited
        ? "Changes saved."
        : result.created
          ? `Saved to ${result.link.categoryName}.`
          : "Already saved. Existing details were kept.",
    );

    if (!edited) {
      setRevealed(result.link);
      setQuery("");
      void load("");
    } else {
      setRevealed(null);
      void load(query);
    }
    void loadCategories();
  }

  useEffect(() => {
    if (capture)
      document.documentElement.dataset.captureHighlight = String(
        capture.settings.highlight,
      );
  }, [capture]);
  const receiveCapture = useEffectEvent((event: CaptureEvent) => {
    setCaptureError("");
    if (event.kind === "settings") {
      setPreferencesOpen(true);
    } else if (event.kind === "saved") {
      const message = event.result.created
        ? `Saved to ${event.result.link.categoryName}.`
        : "Already saved. Existing details were kept.";
      setStatus(message);
      if (event.result.created) {
        void load(query);
        void loadCategories();
      }
    } else if (event.kind === "error") {
      setStatus("");
      setCaptureError(errorMessage(event.error));
    }
  });

  useEffect(() => {
    let active = true;
    void connectCapture((event) => {
      if (active) receiveCapture(event);
    }).then(
      (connection) => {
        if (active) {
          setCapture(connection);
          if (connection.showSettings) setPreferencesOpen(true);
        }
      },
      () => {
        if (active)
          setCaptureSetupError(
            "Capture could not start. Reopen Linkme to retry.",
          );
      },
    );
    return () => {
      active = false;
    };
  }, []);

  async function nativeAction(action: "copy" | "open", id: number) {
    if (nativeRef.current) return;
    nativeRef.current = true;
    setNativeBusy(true);
    setActionError("");
    setStatus("");

    try {
      await (action === "copy" ? copyLink(id) : openLink(id));
      setStatus(
        action === "copy" ? "URL copied." : "Opened in your default browser.",
      );
    } catch (error: unknown) {
      setActionError(errorMessage(error));
    } finally {
      nativeRef.current = false;
      setNativeBusy(false);
    }
  }

  async function confirmDelete() {
    if (!deleting || deleteRef.current) return;
    deleteRef.current = true;
    setDeletePending(true);
    setDeleteError("");

    try {
      await deleteLink(deleting.id, deleting.revision);
      const row = document.querySelector<HTMLElement>(`[data-link-id="${deleting.id}"]`);
      const anchor = (row?.nextElementSibling ?? row?.previousElementSibling
        ?? row?.closest("section.tree-branch")?.querySelector(".branch-label")
        ?? row?.closest(".category-group")?.querySelector(".branch-label")) as HTMLElement | null;
      afterDelete.current = { anchor, top: anchor?.getBoundingClientRect().top ?? 0 };
      request.current++;
      setLoading(false);
      setLinks((old) => old.filter((l) => l.id !== deleting.id));
      setRevealed((old) => (old?.id === deleting.id ? null : old));
      setDeleting(null);
      setStatus("Link deleted.");
    } catch (error: unknown) {
      setDeleteError(errorMessage(error));
    } finally {
      deleteRef.current = false;
      setDeletePending(false);
    }
  }

  const visible =
    !query.trim() && revealed
      ? (links.some(link => link.id === revealed.id) ? mergeLinks(links, [revealed]) : [revealed, ...links])
      : links;

  const [arranging,setArranging] = useState(false);
  const row = (link: Link, handle?: React.ReactNode, rowProps?: React.HTMLAttributes<HTMLLIElement>) => (
    <LinkRow
      key={link.id}
      link={link}
      categoryIcon={categories.find((c) => c.id === link.categoryId)?.icon}
      nativeBusy={nativeBusy}
      rowProps={rowProps}
      handle={handle}
      previewsEnabled={previewsEnabled && !preferencesOpen && !editor && !editingCategory && !deleting}
      onEdit={(link, category = false) => {
        setActionError("");
        setEditor({ link, category });
      }}
      onDelete={(link) => {
        setDeleteError("");
        setDeleting(link);
      }}
      onNative={(action, id) => {
        void nativeAction(action, id);
      }}
    />
  );

  return (
    <main ref={library}>
      <header className="library-header">
        <div className="library-heading">
          <h1>Linkme</h1>
          {drive?.profile && (
            <div className="library-account" title={drive.profile.name}>
              <DriveAccount profile={drive.profile} />
            </div>
          )}
        </div>
        <div className="library-toolbar">
          <label className="sr-only" htmlFor="search">
            Search links
          </label>
          <input
            ref={search}
            id="search"
            name="search"
            className="search"
            type="search"
            placeholder="Search titles, URLs, notes, categories…"
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(e) => {
              setRevealed(null);
              setQuery(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setRevealed(null);
                setQuery("");
              }
            }}
          />
          <div className="header-actions">
            <button className="secondary organize-toggle" aria-pressed={arranging} disabled={!!query.trim()} title={query.trim() ? "Clear search to organize links" : arranging ? "Finish organizing" : "Arrange categories, groups and links"} onClick={() => setArranging(!arranging)}><SymbolIcon name={arranging ? "check" : "organize"} /> {arranging ? "Done" : "Organize"}</button>
            <button
              className="secondary"
              disabled={!capture}
              onClick={() => setPreferencesOpen(true)}
            >
              <SymbolIcon name="settings" /> Capture settings
            </button>
            <button
              disabled={categoryLoading || !!categoryError}
              onClick={() => {
                setActionError("");
                setEditor({ link: null, category: false });
              }}
            >
              <SymbolIcon name="add" /> Add link
            </button>
          </div>
        </div>
      </header>

      <p className="status" role="status">
        {status}
      </p>
      {(captureSetupError || capture?.warning || captureError) && (
        <p role="alert" className="error">
          {captureError || captureSetupError || capture?.warning}
        </p>
      )}

      {(pageError || categoryError || actionError) && (
        <div className="library-error">
          <p className="error" role="alert">
            {pageError || categoryError || actionError}
          </p>
          <button className="secondary" onClick={reload}>
            Reload library
          </button>
        </div>
      )}

      <section aria-label="Saved links" aria-busy={loading}>
        {loading && <p className="hint">Loading links…</p>}

        {!loading && !pageError && visible.length === 0 && (
          <div className="empty">
            <h2>{query.trim() ? "No matching links" : "No links yet"}</h2>
            <p>
              {query.trim()
                ? "Try a different title, URL, note or category."
                : "Add a link to start your Inbox."}
            </p>
          </div>
        )}

        <LibraryTree categories={categories} links={visible} arranging={arranging}
          searching={!!query.trim()} row={row} edit={setEditingCategory}
          onOrdered={(source, target, before) => {
            setLinks(current => {
              const members = current.filter(l => l.categoryId === source.categoryId && l.groupId === source.groupId);
              if (!members.some(l => l.id === source.id) || !members.some(l => l.id === target)) return current;
              const ordered = members.filter(l => l.id !== source.id);
              ordered.splice(ordered.findIndex(l => l.id === target) + Number(!before), 0, members.find(l => l.id === source.id)!);
              let index = 0;
              return current.map(l => l.categoryId === source.categoryId && l.groupId === source.groupId ? ordered[index++] : l);
            });
            if (revealed) { setRevealed(null); void load(query); }
          }}
          moveLink={link => setEditor({link, category:true})}
          reload={() => { setRevealed(null); reload(); }} report={setActionError} />

        {next && (
          <button
            className="secondary"
            disabled={loading}
            onClick={() => {
              void load(query, next);
            }}
          >
            Load older links
          </button>
        )}
      </section>

      {editingCategory && (
        <CategoryEditor
          category={editingCategory}
          onClose={() => setEditingCategory(null)}
          onSaved={() => {
            setEditingCategory(null);
            setRevealed(null);
            setStatus("Category updated.");
            reload();
          }}
        />
      )}
      {editor && (
        <LinkEditor
          link={editor.link}
          categories={categories}
          focusCategory={editor.category}
          onSaved={(result, edited) => {
            setEditor(null);
            saved(result, edited);
          }}
          onCancel={() => setEditor(null)}
        />
      )}

      {preferencesOpen && capture && (
        <CapturePreferences
          settings={capture.settings}
          driveStatus={drive}
          onDriveChanged={setDrive}
          onAppearanceSaved={setCapture}
          onRestored={() => {
            setPreferencesOpen(false);
            setRevealed(null);
            setQuery("");
            setStatus("Backup restored. A local recovery copy was saved.");
            void load("");
            void loadCategories();
            search.current?.focus();
          }}
          previewsEnabled={previewsEnabled}
          onPreviewsChanged={setPreviewsEnabled}
          onSaved={(connection) => {
            setCapture(connection);
            setCaptureError("");
            setPreferencesOpen(false);
          }}
          onClose={() => setPreferencesOpen(false)}
        />
      )}

      {updateNotice}
      {deleting && (
        <ConfirmDialog
          title="Delete link?"
          confirmLabel="Delete link"
          cancelLabel="Keep link"
          pending={deletePending}
          error={deleteError}
          onConfirm={() => {
            void confirmDelete();
          }}
          onCancel={() => setDeleting(null)}
        >
          <p>
            Delete <strong>{deleting.title || deleting.url}</strong> from{" "}
            {deleting.categoryName}?
          </p>
          <p>This cannot be undone.</p>
        </ConfirmDialog>
      )}
    </main>
  );
}
