import { symbolPaths } from "./symbols";
export function SymbolIcon({
  name,
  className = "",
}: {
  name: keyof typeof symbolPaths;
  className?: string;
}) {
  return (
    <svg
      className={"ui-symbol " + className}
      aria-hidden="true"
      focusable="false"
      viewBox="0 0 24 24"
    >
      <path d={symbolPaths[name]} />
    </svg>
  );
}
export function LinkMark() {
  return <SymbolIcon name="link" className="link-mark" />;
}

export function LinkChip({ url }: { url: string }) {
  const parsed = URL.parse(url);
  const host =
    parsed?.protocol === "https:" || parsed?.protocol === "http:"
      ? parsed.hostname
      : null;

  return (
    <span className="link-chip">
      <LinkMark />
      {host && <span className="link-chip-host">{host}</span>}
    </span>
  );
}

export function ReceivingCapsule({
  created,
  category = "Inbox",
}: {
  created: boolean;
  category?: string;
}) {
  return (
    <p className="receiving-capsule" role="status">
      {created ? <SymbolIcon name="check" /> : <LinkMark />}
      {created ? `Saved to ${category}.` : "Already saved."}
    </p>
  );
}
