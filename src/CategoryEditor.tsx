import { useState } from "react";
import { highlightColors } from "./highlightColors";
import icons from "./category-icons.json";
import sprite from "./assets/category-icons.svg?url";
import { SymbolIcon } from "./GlassDetails";
import { Modal } from "./Modal";
import { errorMessage, updateCategory, type Category } from "./links";

export function CategoryGlyph({ icon = "folder" }: { icon?: string }) {
  if (!icons.includes(icon)) icon = "folder";
  return /^[a-z]/.test(icon) ? (
    <svg className="category-glyph" aria-hidden="true" viewBox="0 0 16 16">
      <use href={sprite + "#" + icon} />
    </svg>
  ) : (
    <span className="category-emoji" aria-hidden="true">
      {icon}
    </span>
  );
}

export function IconPicker({
  value,
  onChange,
  paged = false,
}: {
  value: string;
  onChange: (v: string) => void;
  paged?: boolean;
}) {
  const pageSize = 18;
  const [page, setPage] = useState(() => Math.floor(Math.max(0, icons.indexOf(value)) / pageSize));
  const pageCount = Math.ceil(icons.length / pageSize);
  return (
    <details className="icon-picker" data-paged={paged || undefined}>
      <summary>
        <span className="glyph-preview">
          <CategoryGlyph icon={value} />
        </span>
        <span>Category icon</span>
        <SymbolIcon name="chevron" className="picker-chevron" />
      </summary>
      <fieldset>
        <legend>Choose a symbol or emoji</legend>
        <div className="icon-grid">
          {(paged ? icons.slice(page * pageSize, (page + 1) * pageSize) : icons).map((k) => (
            <label key={k} title={k}>
              <input
                type="radio"
                name="category-icon"
                aria-label={k}
                checked={k === value}
                onChange={() => onChange(k)}
              />
              <CategoryGlyph icon={k} />
            </label>
          ))}
        </div>
        {paged && <div className="popup-pages">
          <button type="button" className="secondary compact" aria-label="Previous icons" aria-disabled={page === 0}
            onClick={() => setPage(current => Math.max(0, current - 1))}>Previous</button>
          <span aria-live="polite">{page + 1} / {pageCount}</span>
          <button type="button" className="secondary compact" aria-label="Next icons" aria-disabled={page + 1 === pageCount}
            onClick={() => setPage(current => Math.min(pageCount - 1, current + 1))}>Next</button>
        </div>}
      </fieldset>
    </details>
  );
}
export function CategoryEditor({
  category,
  onClose,
  onSaved,
}: {
  category: Category;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(category.name),
    [icon, setIcon] = useState(
      category.icon || (category.id === 1 ? "inbox" : "folder"),
    );
  const [color, setColor] = useState<number | null>(category.color ?? null);
  const [pinned, setPinned] = useState(category.pinned ?? false);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Modal
      title="Edit category"
      onCancel={() => {
        if (!busy) onClose();
      }}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          setBusy(true);
          setError("");
          try {
            await updateCategory(category.id, name, icon, color, pinned);
            onSaved();
          } catch (err) {
            setError(errorMessage(err));
            setBusy(false);
          }
        }}
      >
        <fieldset disabled={busy}>
          <label htmlFor="category-name">Category name</label>
          <input
            id="category-name"
            data-initial-focus
            required
            maxLength={80}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <IconPicker value={icon} onChange={setIcon} />
          <fieldset className="category-swatches">
            <legend>Category color</legend>
            {[null, ...highlightColors.map((_, i) => i)].map((index) => {
              const label =
                index === null
                  ? "Match app appearance"
                  : highlightColors[index].split(" ·")[0];
              return (
                <label
                  key={label}
                  data-capture-highlight={index ?? undefined}
                  title={label}
                >
                  <input
                    type="radio"
                    name="category-color"
                    aria-label={label}
                    checked={color === index}
                    onChange={() => setColor(index)}
                  />
                  <span
                    className={index === null ? "swatch auto-swatch" : "swatch"}
                  >
                    {color === index && <SymbolIcon name="check" />}
                  </span>
                </label>
              );
            })}
          </fieldset>
          <button
            type="button"
            className="pin-toggle secondary"
            aria-label="Pin category to top"
            aria-pressed={pinned}
            onClick={() => setPinned(!pinned)}
          >
            <SymbolIcon name="pin" />
            {pinned ? "Pinned to top" : "Pin to top"}
          </button>
          {error && <p role="alert">{error}</p>}
          <div className="dialog-actions">
            <button type="button" className="secondary" onClick={onClose}>
              Cancel
            </button>
            <button type="submit">Save category</button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
