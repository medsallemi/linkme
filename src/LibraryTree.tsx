import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { DragEvent, HTMLAttributes, KeyboardEvent, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CategoryGlyph } from "./CategoryEditor";
import { SymbolIcon } from "./GlassDetails";
import { categorySymbol } from "./WebsiteIcon";
import { errorMessage, orderLink, saveLink } from "./links";
import type { Category, Link } from "./links";
import { Modal } from "./Modal";

type Drag = { category: number } | { link: Link };
type GroupForm = { category: Category; id?: number; name: string };

function DisclosureBody({ open, id, children, className = "" }: {
  open: boolean; id: string; children: ReactNode; className?: string;
}) {
  const [settled, setSettled] = useState(open);
  useLayoutEffect(() => {
    if (open === settled) return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      setSettled(open);
      return;
    }
    const timer = setTimeout(() => setSettled(open), 190);
    return () => clearTimeout(timer);
  }, [open, settled]);
  return <div id={id} className="branch-disclosure" data-open={open} data-moving={open !== settled || undefined}
    inert={!open} aria-hidden={open ? undefined : true}>
    <div className={"branch-disclosure-inner " + className}>{(open || settled) && children}</div>
  </div>;
}

type Props = {
  categories: Category[]; links: Link[]; arranging: boolean; searching: boolean;
  row: (link: Link, handle?: ReactNode, props?: HTMLAttributes<HTMLLIElement>) => ReactNode;
  edit: (category: Category) => void; moveLink: (link: Link) => void;
  onOrdered: (source: Link, target: number, before: boolean) => void;
  reload: () => void; report: (message: string) => void;
};
export function LibraryTree({ categories, links, arranging, searching, row, edit, moveLink, onOrdered, reload, report }: Props) {
  const [collapsed, setCollapsed] = useState(new Set<string>());
  const [form, setForm] = useState<GroupForm | null>(null);
  const [formError, setFormError] = useState("");
  const [pending, setPending] = useState(false);
  const [over, setOver] = useState("");
  const [orderStatus, setOrderStatus] = useState("");
  const [focused, setFocused] = useState(() => document.hasFocus());
  const organizing = arranging && !searching;
  const writingOrder = useRef(false);
  const drag = useRef<Drag | null>(null);
  const tree = useRef<HTMLDivElement>(null);
  const previousTops = useRef<Map<string, number> | null>(null);
  useEffect(() => {
    if (!organizing) return;
    const sync = () => setFocused(document.hasFocus() && !document.hidden);
    sync();
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
    };
  }, [organizing]);
  useLayoutEffect(() => {
    const previous = previousTops.current;
    previousTops.current = null;
    if (!previous || !organizing || !tree.current || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    for (const section of tree.current.querySelectorAll<HTMLElement>("[data-category-id]")) {
      const before = previous.get(section.dataset.categoryId!);
      if (before === undefined) continue;
      const offset = before - section.getBoundingClientRect().top;
      if (Math.abs(offset) > 1) section.animate?.(
        [{ transform: `translateY(${offset}px)` }, { transform: "none" }],
        { duration: 230, easing: "cubic-bezier(.2,.75,.25,1)" },
      );
    }
  }, [categories, organizing]);
  const branches = new Map<number, Map<number, Link[]>>();
  for (const link of links) {
    const group = link.groupId ?? 0;
    const branch = branches.get(link.categoryId) ?? new Map<number, Link[]>();
    const members = branch.get(group) ?? [];
    members.push(link);
    branch.set(group, members);
    branches.set(link.categoryId, branch);
  }
  function disclosure(key: string, label: ReactNode) {
    const expanded = searching || !collapsed.has(key);
    return <button type="button" className="branch-label" aria-expanded={expanded} aria-controls={"branch-" + key}
      aria-disabled={searching || undefined} onClick={() => {
        if (searching) return;
        const next = new Set(collapsed);
        if (expanded) next.add(key); else next.delete(key);
        setCollapsed(next);
      }}><SymbolIcon name="chevron" />{label}</button>;
  }
  function endDrag() { drag.current = null; setOver(""); }
  function handle(value: Drag, label: string, onKeyDown?: (event: KeyboardEvent) => void) {
    if (!organizing) return null;
    const Handle = "link" in value ? "button" : "span";
    return <Handle draggable tabIndex={"category" in value ? 0 : undefined} role={"category" in value ? "group" : undefined} className="tree-grip" onKeyDown={onKeyDown}
      aria-keyshortcuts={onKeyDown ? "Alt+ArrowUp Alt+ArrowDown" : undefined}
      title={"link" in value ? "Drag to arrange · Click to move · Alt+↑ / Alt+↓ to reorder" : label}
      aria-label={"link" in value ? "Move " + (value.link.title || value.link.url) : label}
      onClick={"link" in value ? () => moveLink(value.link) : undefined}
      onDragStart={event => {
        drag.current = value;
        event.dataTransfer.setData("text/plain", label);
        event.dataTransfer.effectAllowed = "move";
      }}
      onDragEnd={endDrag}><SymbolIcon name="grip" /></Handle>;
  }
  async function reorder(source: number, target: number) {
    if (source === target) return;
    const items = categories.map(c => c.id);
    const before = items.indexOf(source), after = items.indexOf(target);
    if (before < 0 || after < 0 || !!categories[before].pinned !== !!categories[after].pinned) return;
    items.splice(before, 1);
    items.splice(after, 0, source);
    try {
      await invoke("order_categories", { ids: items });
      if (tree.current && !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
        previousTops.current = new Map(Array.from(
          tree.current.querySelectorAll<HTMLElement>("[data-category-id]"),
          section => [section.dataset.categoryId!, section.getBoundingClientRect().top],
        ));
      }
      reload();
    } catch (error) { report(errorMessage(error)); }
  }
  function dropZone(category: Category, group: number | null, key: string) {
    function accepts() {
      const source = drag.current;
      return organizing && source && ("link" in source ||
        (group === null && !!category.pinned === !!categories.find(c => c.id === source.category)?.pinned));
    }
    return {
      "data-drop": over === key || undefined,
      onDragOver: (event: DragEvent) => {
        if (!accepts()) return;
        event.preventDefault(); event.stopPropagation(); setOver(key);
      },
      onDragLeave: (event: DragEvent) => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget))
          setOver(current => current === key ? "" : current);
      },
      onDrop: async (event: DragEvent) => {
        if (!accepts()) return;
        event.preventDefault(); event.stopPropagation();
        const source = drag.current;
        endDrag();
        if (!source) return;
        if ("category" in source) return reorder(source.category, category.id);
        const link = source.link;
        if (link.categoryId === category.id && (link.groupId ?? null) === group) return;
        try {
          await saveLink({ id: link.id, revision: link.revision, url: link.url, title: link.title,
            subtitle: link.subtitle, caption: link.caption, categoryId: category.id, groupId: group, newCategory: null });
          reload();
        } catch (error) { report(errorMessage(error)); }
      },
    };
  }
  async function place(source: Link, target: Link, before: boolean) {
    if (writingOrder.current || source.id === target.id) return;
    writingOrder.current = true;
    try {
      await orderLink(source.id, target.id, before);
      report("");
      onOrdered(source, target.id, before);
      setOrderStatus(`Moved ${source.title || source.url} ${before ? "before" : "after"} ${target.title || target.url}.`);
    } catch (error) { report(errorMessage(error)); }
    finally { writingOrder.current = false; }
  }
  function rows(items: Link[] = []) {
    return <ul className="links">{items.map((link, index) => {
      if (!organizing) return row(link);
      const grip = handle({ link }, "Drag to reorder or move link", event => {
        if (!event.altKey || !["ArrowUp", "ArrowDown"].includes(event.key)) return;
        event.preventDefault();
        const up = event.key === "ArrowUp";
        const neighbor = items[index + (up ? -1 : 1)];
        if (neighbor) void place(link, neighbor, up);
      });
      const before = (event: DragEvent<HTMLLIElement>) => {
        const rect = event.currentTarget.getBoundingClientRect();
        return event.clientY < rect.top + rect.height / 2;
      };
      function sibling() {
        const source = drag.current;
        return source && "link" in source && source.link.id !== link.id &&
          source.link.categoryId === link.categoryId && source.link.groupId === link.groupId ? source.link : null;
      }
      const top = "before" + link.id, bottom = "after" + link.id;
      return row(link, grip, {
        className: "link-row" + (over === top ? " insert-before" : over === bottom ? " insert-after" : ""),
        onDragOver(event) {
          if (!sibling()) return;
          event.preventDefault(); event.stopPropagation(); setOver(before(event) ? top : bottom);
        },
        onDragLeave(event) {
          if (!event.currentTarget.contains(event.relatedTarget instanceof Node ? event.relatedTarget : null))
            setOver(current => current === top || current === bottom ? "" : current);
        },
        onDrop(event) {
          const source = sibling();
          if (!source) return;
          event.preventDefault(); event.stopPropagation(); endDrag();
          void place(source, link, before(event));
        }
      });
    })}</ul>;
  }
  function openForm(value: GroupForm) { setFormError(""); setForm(value); }
  const ordered = searching ? Array.from(branches.keys()).map(id => categories.find(c => c.id === id)).filter((c): c is Category => !!c) : categories;
  return <div ref={tree} className="library-tree" data-arranging={organizing || undefined} data-motion={focused || undefined}>
    <span className="sr-only" aria-live="polite">{orderStatus}</span>
    {ordered.map(category => {
      const key = "c" + category.id;
      const branch = branches.get(category.id);
      const tier = categories.filter(c => !!c.pinned === !!category.pinned);
      const index = tier.findIndex(c => c.id === category.id);
      return <section key={key} className="category-group" data-category-id={category.id} data-pinned={category.pinned || undefined}
        data-capture-highlight={category.color ?? undefined} aria-labelledby={"category-" + category.id}
        {...dropZone(category, null, key)}>
        <div className="category-header">
          {handle({ category: category.id }, "Reorder " + category.name + " · Drag or use Alt+Up / Alt+Down", event => {
            if (!event.altKey || !["ArrowUp", "ArrowDown"].includes(event.key)) return;
            event.preventDefault();
            const neighbor = tier[index + (event.key === "ArrowUp" ? -1 : 1)];
            if (neighbor) void reorder(category.id, neighbor.id);
          })}
          <h2 id={"category-" + category.id}>{disclosure(key, <>
            <span className="collection-mark"><CategoryGlyph icon={category.icon || categorySymbol(category.name)} /></span><span>{category.name}</span>
            {category.pinned && <span className="pin-seal" title="Pinned"><SymbolIcon name="pin" /><span className="sr-only">Pinned</span></span>}
          </>)}</h2>
          {organizing && <div className="arrange-tools">
            <button className="secondary compact" aria-label={"Add group to " + category.name} title="Add group" onClick={() => openForm({ category, name: "" })}><SymbolIcon name="add" /> Group</button>
          </div>}
          <button className="edit-category secondary compact" title="Edit category" aria-label={"Edit category " + category.name} onClick={() => edit(category)}><SymbolIcon name="edit" /></button>
        </div>
        <DisclosureBody id={"branch-" + key} open={searching || !collapsed.has(key)} className="category-contents">
          {rows(branch?.get(0))}
          {(category.groups ?? []).filter(g => !searching || branch?.has(g.id)).map(g => {
            const groupKey = "g" + g.id;
            return <section className="tree-branch" key={groupKey} {...dropZone(category,g.id,groupKey)}>
              <div className="group-heading">
                <h3>{disclosure(groupKey,<><SymbolIcon name="folder" />{g.name}</>)}</h3>
                {organizing && <button className="edit-category secondary compact" title="Rename group" aria-label={"Rename group " + g.name} onClick={() => openForm({ category, id:g.id, name:g.name })}><SymbolIcon name="edit" /></button>}
              </div>
              <DisclosureBody id={"branch-" + groupKey} open={searching || !collapsed.has(groupKey)}>{rows(branch?.get(g.id))}</DisclosureBody>
            </section>;
          })}
        </DisclosureBody>
      </section>;
    })}
    {form && <Modal title={form.id ? "Rename group" : "New group"} onCancel={() => { if (!pending) setForm(null); }}>
      <form onSubmit={async e => {
        e.preventDefault(); if (pending) return; setPending(true);
        try {
          await invoke("save_group",{ category:form.category.id,id:form.id ?? null,name:form.name.trim() });
          setCollapsed(old => { const next = new Set(old); next.delete("c"+form.category.id); return next; });
          setForm(null); reload();
        } catch(error) { setFormError(errorMessage(error)); } finally { setPending(false); }
      }}>
        <label htmlFor="group-name">Group name in {form.category.name}</label>
        <input id="group-name" data-initial-focus maxLength={80} required value={form.name} onChange={e=>setForm({...form,name:e.target.value})}/>
        {formError && <p role="alert" className="error">{formError}</p>}
        <div className="dialog-actions"><button type="button" className="secondary" disabled={pending} onClick={()=>setForm(null)}>Cancel</button><button disabled={pending || !form.name.trim()}>{form.id ? "Save" : "Create group"}</button></div>
      </form>
    </Modal>}
  </div>;
}
