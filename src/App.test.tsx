import { beforeEach, expect, it, vi } from "vitest";
import { act } from "@testing-library/react";
import { connectCapture, resetLinkme, updateCapture } from "./capture";
import type { CaptureEvent } from "./capture";

let captureEvent: (event: CaptureEvent) => void;
vi.mock("./capture", () => ({
  connectCapture: vi.fn(),
  updateCapture: vi.fn(),
  resetLinkme: vi.fn(),
}));

import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";

import App from "./App";
import { driveBackup } from "./DriveBackup";

vi.mock("./DriveBackup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./DriveBackup")>()),
  driveBackup: vi.fn(),
}));

import { ErrorBoundary } from "./ErrorBoundary";

import {
  orderLink,
  copyLink,
  deleteLink,
  listCategories,
  listLinks,
  openLink,
  saveLink,
  updateCategory,
} from "./links";

import type { Link, LinkPage } from "./links";

vi.mock("./links", async (original) => {
  const actual = await original<typeof import("./links")>();
  return {
    ...actual,
    listLinks: vi.fn(),
    listCategories: vi.fn(),
    saveLink: vi.fn(),
    updateCategory: vi.fn(),
    deleteLink: vi.fn(),
    orderLink: vi.fn(),
    copyLink: vi.fn(),
    openLink: vi.fn(),
  };
});

const list = vi.mocked(listLinks),
  categories = vi.mocked(listCategories),
  save = vi.mocked(saveLink),
  remove = vi.mocked(deleteLink),
  copy = vi.mocked(copyLink),
  open = vi.mocked(openLink);

const inbox = { id: 1, name: "Inbox" };

function link(extra: Partial<Link> = {}): Link {
  return {
    id: 1,
    url: "https://example.com/kept",
    title: "Kept title",
    subtitle: "Kept subtitle",
    caption: "Kept notes",
    categoryId: 1,
    categoryName: "Inbox",
    revision: 0,
    ...extra,
  };
}

function page(links: Link[], next: LinkPage["next"] = null): LinkPage {
  return { links, next };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(driveBackup).mockResolvedValue({ configured: true, requiresReconnect: false, profile: null, frequency: "off", lastBackup: null, warning: null });
  list.mockResolvedValue(page([]));
  categories.mockResolvedValue([inbox]);
  copy.mockResolvedValue();
  open.mockResolvedValue();
  remove.mockResolvedValue();
  vi.mocked(connectCapture).mockImplementation(async (receive) => {
    captureEvent = receive;
    return {
      settings: {
        mode: "prompt",
        shortcut: "Ctrl+Shift+L",
        searchShortcut: "Ctrl+Shift+K",
        dimPercent: 35,
        panelTransparency: 18,
        highlight: 0,
      },
      warning: null,
    };
  });
});

it("does not open an editor in the library for a native Prompt event", async () => {
  render(<App />);
  await waitFor(() => expect(connectCapture).toHaveBeenCalled());
  const url = "https://example.com/capture?x=%2F#Exact";
  act(() => captureEvent({ kind: "prompt", url }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(save).not.toHaveBeenCalled();
});

it("keeps an open draft through Quick-save feedback without submitting it", async () => {
  render(<App />);
  const dialog = await add();
  field("URL", "https://example.com/manual-draft");
  act(() =>
    captureEvent({ kind: "saved", result: { link: link(), created: false } }),
  );
  expect(
    (within(dialog).getByLabelText("URL") as HTMLTextAreaElement).value,
  ).toBe("https://example.com/manual-draft");
  expect(
    screen.getByText("Already saved. Existing details were kept."),
  ).toBeTruthy();
  fireEvent(window, new Event("linkme-library-open"));
  expect(
    screen.queryByText("Already saved. Existing details were kept."),
  ).toBeNull();
  expect(save).not.toHaveBeenCalled();
});

it("leaves shortcut settings editable after a registration conflict", async () => {
  vi.mocked(updateCapture)
    .mockRejectedValueOnce({
      code: "shortcut_conflict",
      message: "That shortcut is unavailable.",
    })
    .mockResolvedValueOnce({
      settings: {
        mode: "quick",
        shortcut: "Ctrl+Shift+J",
        searchShortcut: "Ctrl+Shift+K",
        dimPercent: 32,
        panelTransparency: 0,
        highlight: 5,
      },
      warning: null,
    });
  render(<App />);
  const preferences = screen.getByRole("button", { name: "Capture settings" });
  await waitFor(() =>
    expect((preferences as HTMLButtonElement).disabled).toBe(false),
  );
  preferences.focus();
  fireEvent.click(preferences);
  const dialog = screen.getByRole("dialog", { name: "Capture settings" });
  field("Capture mode", "quick");
  field("Capture shortcut", "Ctrl+Shift+Y");
  field("Popup background dimming", "32");
  field("Floating panel transparency", "70");
  expect(
    screen
      .getByRole("slider", { name: "Floating panel transparency" })
      .getAttribute("aria-valuetext"),
  ).toBe("70%");
  field("Floating panel transparency", "0");
  expect(screen.getAllByRole("radio")).toHaveLength(8);
  expect(
    screen
      .getByRole("slider", { name: "Popup background dimming" })
      .getAttribute("aria-valuetext"),
  ).toBe("32%");
  expect(
    screen
      .getByRole("slider", { name: "Floating panel transparency" })
      .getAttribute("aria-valuetext"),
  ).toBe("Solid");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Save settings" }),
  );
  expect(await within(dialog).findByRole("alert")).toHaveProperty(
    "textContent",
    "That shortcut is unavailable.",
  );
  expect(
    (within(dialog).getByLabelText("Capture shortcut") as HTMLInputElement)
      .value,
  ).toBe("Ctrl+Shift+Y");
  field("Capture shortcut", "Ctrl+Shift+J");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Save settings" }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(updateCapture).toHaveBeenLastCalledWith(
    expect.objectContaining({
      dimPercent: 32,
      panelTransparency: 0,
      highlight: 0,
    }),
  );
  expect(document.activeElement).toBe(preferences);
});

async function add() {
  const button = screen.getByRole("button", { name: "Add link" });
  await waitFor(() =>
    expect((button as HTMLButtonElement).disabled).toBe(false),
  );
  button.focus();
  fireEvent.click(button);
  return screen.getByRole("dialog", { name: "Add link" });
}

it("applies the accent immediately and saves it without committing the shortcut draft", async () => {
  const connection = {
    settings: {
      mode: "prompt" as const, shortcut: "Ctrl+Shift+L", searchShortcut: "Ctrl+Shift+K",
      dimPercent: 40, panelTransparency: 25, highlight: 0,
    },
    warning: null,
  };
  vi.mocked(connectCapture).mockResolvedValue(connection);
  vi.mocked(updateCapture).mockImplementation(async settings => ({ settings, warning: null }));
  render(<App />);
  const open = screen.getByRole<HTMLButtonElement>("button", { name: "Capture settings" });
  await waitFor(() => expect(open.disabled).toBe(false));
  fireEvent.click(open);
  field("Capture shortcut", "Ctrl+Shift+Z");
  const coffee = screen.getByRole<HTMLInputElement>("radio", { name: "Creamy Coffee" });
  fireEvent.click(coffee);
  expect(document.documentElement.dataset.captureHighlight).toBe(coffee.value);
  await waitFor(() => expect(updateCapture).toHaveBeenCalledOnce());
  expect(updateCapture).toHaveBeenCalledWith(expect.objectContaining({
    shortcut: "Ctrl+Shift+L", highlight: Number(coffee.value),
  }));
  expect(screen.getByRole("dialog", { name: "Capture settings" })).toBeTruthy();
  expect((screen.getByLabelText("Capture shortcut") as HTMLInputElement).value).toBe("Ctrl+Shift+Z");
});

function field(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

async function rowActions(title = "Kept title") {
  const button = await screen.findByRole("button", { name: `Open ${title}` });
  const row = button.closest("li")!;
  fireEvent.click(within(row).getByLabelText(/^Actions for/));
  return row;
}

async function edit() {
  const row = await rowActions();
  fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
  return screen.getByRole("dialog", { name: "Edit link" });
}

it("loads existing metadata grouped under its category without fetching websites", async () => {
  list.mockResolvedValue(
    page([
      link(),
      link({
        id: 2,
        url: "https://example.com/reading",
        title: "Reading title",
        categoryId: 2,
        categoryName: "Reading",
      }),
    ]),
  );
  categories.mockResolvedValue([inbox, { id: 2, name: "Reading" }]);
  render(<App />);

  expect(await screen.findByRole("heading", { name: "Reading" })).toBeTruthy();
  expect(
    within(
      (await screen.findByRole("button", { name: "Open Kept title" })).closest(
        "li",
      )!,
    ).getByText("Kept subtitle"),
  ).toBeTruthy();
  expect(screen.queryByRole("link")).toBeNull();
  expect(screen.queryByText("Kept notes")).toBeNull();
});

it("creates a link with optional metadata and a new category in one request", async () => {
  const created = link({
    id: 2,
    title: "New title",
    subtitle: "New subtitle",
    caption: "Notes",
    categoryId: 2,
    categoryName: "Reading",
  });
  save.mockResolvedValue({ link: created, created: true });
  render(<App />);
  await add();
  field("URL", created.url);
  field("Title (optional)", "New title");
  field("Subtitle (optional)", "New subtitle");
  field("Caption (optional)", "Notes");
  field("Category", "__new__");
  field("New category name", "Reading");
  categories.mockResolvedValue([inbox, { id: 2, name: "Reading" }]);
  fireEvent.click(screen.getByRole("button", { name: "Save link" }));

  expect(await screen.findByText("Saved to Reading.")).toBeTruthy();
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      newCategory: "Reading",
      title: "New title",
      subtitle: "New subtitle",
      caption: "Notes",
      id: null,
    }),
  );
  expect(await screen.findByRole("heading", { name: "Reading" })).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("waits for commit, keeps the draft on failure, and prevents double submits", async () => {
  let reject!: (e: unknown) => void;
  save.mockReturnValue(
    new Promise((_, r) => {
      reject = r;
    }),
  );
  render(<App />);
  const dialog = await add();
  field("URL", "https://example.com/new");
  fireEvent.submit(dialog.querySelector("form")!);
  fireEvent.submit(dialog.querySelector("form")!);
  expect(save).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("Saved to Inbox.")).toBeNull();
  expect(
    (screen.getByRole("button", { name: "Saving…" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  reject({ code: "database", message: "private db path" });
  expect((await screen.findByRole("alert")).textContent).toContain(
    "database is unavailable",
  );
  expect((screen.getByLabelText("URL") as HTMLTextAreaElement).value).toBe(
    "https://example.com/new",
  );
  expect(screen.queryByText("private db path")).toBeNull();
});

it("shows duplicates without treating submitted details as an overwrite", async () => {
  save.mockResolvedValue({ link: link(), created: false });
  render(<App />);
  await add();
  field("URL", link().url);
  field("Title (optional)", "Do not overwrite");
  fireEvent.click(screen.getByRole("button", { name: "Save link" }));
  expect(
    await screen.findByText("Already saved. Existing details were kept."),
  ).toBeTruthy();
  expect(
    await screen.findByRole("button", { name: "Open Kept title" }),
  ).toBeTruthy();
  expect(screen.queryByText("Do not overwrite")).toBeNull();
});

it("preserves multiline URLs for Rust rejection and supports Ctrl+Enter", async () => {
  save.mockRejectedValue({
    code: "invalid_url",
    message: "Remove control characters from the URL.",
  });
  render(<App />);
  await add();
  const raw = "https://example.com/one\nhttps://example.com/two";
  field("URL", raw);
  fireEvent.keyDown(screen.getByLabelText("URL"), { key: "Enter" });
  expect(save).not.toHaveBeenCalled();
  fireEvent.keyDown(screen.getByLabelText("URL"), {
    key: "Enter",
    ctrlKey: true,
  });
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Remove control");
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ url: raw }));
  expect(document.activeElement).toBe(alert);
});

it("edits prefilled metadata and category without losing other fields", async () => {
  list.mockResolvedValue(page([link()]));
  categories.mockResolvedValue([inbox, { id: 2, name: "Reading" }]);
  save.mockResolvedValue({
    link: link({
      title: "Updated",
      categoryId: 2,
      categoryName: "Reading",
      revision: 1,
    }),
    created: false,
  });
  render(<App />);
  await edit();
  expect(
    (screen.getByLabelText("Caption (optional)") as HTMLTextAreaElement).value,
  ).toBe("Kept notes");
  field("Title (optional)", "Updated");
  field("Category", "2");
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  expect(await screen.findByText("Changes saved.")).toBeTruthy();
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: 1,
      revision: 0,
      title: "Updated",
      subtitle: "Kept subtitle",
      caption: "Kept notes",
      categoryId: 2,
    }),
  );
});

it("retains the edit draft on a stale revision or URL collision", async () => {
  list.mockResolvedValue(page([link()]));
  save.mockRejectedValue({
    code: "conflict",
    message: "This link changed in another window.",
  });
  render(<App />);
  await edit();
  field("Title (optional)", "My draft");
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "another window",
  );
  expect(
    (screen.getByLabelText("Title (optional)") as HTMLInputElement).value,
  ).toBe("My draft");
  expect(screen.getByRole("dialog", { name: "Edit link" })).toBeTruthy();
});

it("canceling a dirty editor asks before discarding and saves nothing", async () => {
  render(<App />);
  const addButton = screen.getByRole("button", { name: "Add link" });
  await add();
  field("URL", "https://example.com/draft");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  const confirm = screen.getByRole("dialog", {
    name: "Discard unsaved changes?",
  });
  expect(document.activeElement).toBe(
    within(confirm).getByRole("button", { name: "Keep editing" }),
  );
  fireEvent.click(
    within(confirm).getByRole("button", { name: "Keep editing" }),
  );
  expect((screen.getByLabelText("URL") as HTMLTextAreaElement).value).toContain(
    "draft",
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(save).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(addButton);
});

it("never deletes before explicit confirmation and restores focus on cancel", async () => {
  list.mockResolvedValue(page([link()]));
  render(<App />);
  const row = await rowActions();
  fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
  const dialog = screen.getByRole("dialog", { name: "Delete link?" });
  expect(remove).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(
    within(dialog).getByRole("button", { name: "Keep link" }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Keep link" }));
  expect(remove).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Open Kept title" })).toBeTruthy();
  expect(document.activeElement).toBe(
    within(row).getByLabelText(/^Actions for/),
  );
});

it("deletes the confirmed revision and focuses the nearest remaining row without reloading", async () => {
  list.mockResolvedValue(page([link(), link({ id: 2, title: "Next title" })]));
  render(<App />);
  const row = await rowActions();
  fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete link" }));
  expect(await screen.findByText("Link deleted.")).toBeTruthy();
  expect(remove).toHaveBeenCalledWith(1, 0);
  expect(screen.queryByRole("button", { name: "Open Kept title" })).toBeNull();
  expect(list).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Open Next title" }));
});

it("keeps a failed deletion visible without removing the saved row", async () => {
  list.mockResolvedValue(page([link()]));
  remove.mockRejectedValue({
    code: "conflict",
    message: "This link changed. Reload it.",
  });
  render(<App />);
  const row = await rowActions();
  fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete link" }));
  expect((await screen.findByRole("alert")).textContent).toContain("Reload it");
  expect(screen.getByRole("dialog", { name: "Delete link?" })).toBeTruthy();
});

it("copy and open send saved IDs and show native failures safely", async () => {
  list.mockResolvedValue(page([link()]));
  render(<App />);
  const copyButton = await screen.findByRole("button", {
    name: "Copy URL for Kept title",
  });
  fireEvent.click(copyButton);
  expect(await screen.findByText("URL copied.")).toBeTruthy();
  expect(copy).toHaveBeenCalledWith(1);
  fireEvent.click(screen.getByRole("button", { name: "Open Kept title" }));
  expect(
    await screen.findByText("Opened in your default browser."),
  ).toBeTruthy();
  expect(open).toHaveBeenCalledWith(1);
  copy.mockRejectedValueOnce(new Error("secret native stack"));
  fireEvent.click(copyButton);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "could not be confirmed",
  );
  expect(screen.queryByText("secret native stack")).toBeNull();
});

it("does not let an older search response replace newer results", async () => {
  let old!: (p: LinkPage) => void;
  list.mockImplementation((q) =>
    q === "old"
      ? new Promise((r) => {
          old = r;
        })
      : Promise.resolve(
          page(q === "new" ? [link({ title: "New result" })] : []),
        ),
  );
  render(<App />);
  await screen.findByText("No links yet");
  field("Search links", "old");
  await waitFor(() => expect(list).toHaveBeenCalledWith("old", null));
  field("Search links", "new");
  expect(
    await screen.findByRole("button", { name: "Open New result" }),
  ).toBeTruthy();
  old(page([link({ title: "Stale result" })]));
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "Open Stale result" }),
    ).toBeNull(),
  );
  expect(screen.getByRole("button", { name: "Open New result" })).toBeTruthy();
});

it("uses literal search text, cursor paging, and retains rows after a page failure", async () => {
  const cursor = { rank: 2, id: 101 };
  list
    .mockResolvedValueOnce(page([link()], cursor))
    .mockRejectedValueOnce({ code: "database" });
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Load older links" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Could not load links",
  );
  expect(screen.getByRole("button", { name: "Open Kept title" })).toBeTruthy();
  expect(list).toHaveBeenLastCalledWith("", cursor);
  list.mockResolvedValue(page([]));
  field("Search links", "%_notes");
  expect(await screen.findByText("No matching links")).toBeTruthy();
  expect(list).toHaveBeenLastCalledWith("%_notes", null);
});

it("load failures offer retry without displaying private details", async () => {
  list.mockRejectedValueOnce(new Error("private database path"));
  render(<App />);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Could not load links",
  );
  expect(screen.queryByText("private database path")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Reload library" }));
  expect(await screen.findByText("No links yet")).toBeTruthy();
});

it("contains unexpected render failures without showing private details", () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  function Broken(): never {
    throw new Error("private render details");
  }
  render(
    <ErrorBoundary>
      <Broken />
    </ErrorBoundary>,
  );
  expect(screen.getByRole("alert").textContent).toContain(
    "Close and reopen Linkme",
  );
  expect(screen.queryByText("private render details")).toBeNull();
});

it("keeps search relevance order when appending the next page", async () => {
  const cursor = { rank: 1, id: 2 };
  list.mockImplementation((query, before) =>
    Promise.resolve(
      query
        ? before
          ? page([link({ id: 90, title: "Recent substring" })])
          : page(
              [
                link({ id: 1, title: "Exact match" }),
                link({ id: 2, title: "Prefix match" }),
              ],
              cursor,
            )
        : page([]),
    ),
  );
  render(<App />);
  await screen.findByText("No links yet");
  field("Search links", "match");
  fireEvent.click(
    await screen.findByRole("button", { name: "Load older links" }),
  );
  await screen.findByRole("button", { name: "Open Recent substring" });
  expect(
    screen.getAllByRole("button", { name: /^Open / }).map((b) => b.textContent),
  ).toEqual(["Exact match", "Prefix match", "Recent substring"]);
});

it("renames a category and updates its chosen emoji in one save", async () => {
  categories.mockResolvedValue([inbox]);
  list.mockResolvedValue(page([link()]));
  render(<App />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Edit category Inbox" }),
  );
  field("Category name", "Personal");
  fireEvent.click(screen.getByRole("radio", { name: "Peach" }));
  fireEvent.click(screen.getByLabelText("Pin category to top"));
  fireEvent.click(screen.getByText("Category icon"));
  fireEvent.click(screen.getByRole("radio", { name: "🌊" }));
  fireEvent.click(screen.getByRole("button", { name: "Save category" }));
  await waitFor(() =>
    expect(updateCategory).toHaveBeenCalledWith(1, "Personal", "🌊", 3, true),
  );
});

it("restores the library entrance on explicit reopen and honors reduced motion", async () => {
  const originalMedia = window.matchMedia;
  const originalAnimate = HTMLElement.prototype.animate;
  const cancel = vi.fn();
  const animate = vi.fn(() => ({ cancel }) as unknown as Animation);
  let motion = true;
  window.matchMedia = () => ({ matches: motion }) as MediaQueryList;
  HTMLElement.prototype.animate = animate;
  try {
    const view = render(<App />);
    await screen.findByRole("heading", { name: "Linkme" });
    expect(animate).toHaveBeenCalledTimes(1);
    fireEvent(window, new Event("linkme-library-open"));
    expect(animate).toHaveBeenCalledTimes(2);
    expect(cancel).toHaveBeenCalledOnce();
    motion = false;
    fireEvent(window, new Event("linkme-library-open"));
    expect(animate).toHaveBeenCalledTimes(2);
    view.unmount();
  } finally {
    window.matchMedia = originalMedia;
    HTMLElement.prototype.animate = originalAnimate;
  }
});

it("requires a separate confirmation to erase Linkme data", async () => {
  render(<App />);
  await waitFor(() => expect(connectCapture).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Capture settings" }));
  const reset = screen.getByRole("button", { name: "Reset Linkme…" });
  fireEvent.click(reset);
  const dialog = screen.getByRole("dialog", { name: "Reset Linkme?" });
  const cancel = within(dialog).getByRole("button", { name: "Cancel" });
  expect(document.activeElement).toBe(cancel);
  fireEvent.click(cancel);
  expect(resetLinkme).not.toHaveBeenCalled();
  fireEvent.click(reset);
  fireEvent.click(screen.getByRole("button", { name: "Erase data and restart" }));
  expect(resetLinkme).toHaveBeenCalledOnce();
});

it("opens matching groups during search and restores their collapsed state", async () => {
  categories.mockResolvedValue([{ ...inbox, groups: [{ id: 7, name: "Papers" }] }]);
  list.mockResolvedValue(page([link({ title: "Fluid motion", groupId: 7, groupName: "Papers" })]));
  render(<App />);
  const group = await screen.findByRole("button", { name: "Papers" });
  await screen.findByRole("button", {name: "Open Fluid motion"});
  fireEvent.click(group);
  expect(screen.queryByRole("button", {name:"Open Fluid motion"})).toBeNull();
  field("Search links","Papers");
  expect(await screen.findByRole("button", {name:"Open Fluid motion"})).toBeTruthy();
  field("Search links","");
  expect(screen.getByRole("button", {name:"Papers"}).getAttribute("aria-expanded")).toBe("false");
});

it("moves a dragged link into a group without losing metadata", async () => {
  const original = link({title:"Reading list", caption:"Keep this note"});
  categories.mockResolvedValue([{ ...inbox, groups: [{id: 9, name:"Archive"}] }]);
  list.mockResolvedValue(page([original]));
  save.mockResolvedValue({created:false, link:{...original, groupId:9}});
  render(<App />);
  await screen.findByRole("button", {name:"Open Reading list"});
  fireEvent.click(screen.getByRole("button", {name:"Organize"}));
  const grip = screen.getByRole("button", {name:"Move Reading list"});
  const target = screen.getByRole("button", {name:"Archive"});
  const dataTransfer = { setData:vi.fn(), effectAllowed:"" };
  fireEvent.dragStart(grip,{dataTransfer});
  fireEvent.dragOver(target,{dataTransfer});
  fireEvent.drop(target,{dataTransfer});
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({
    id:original.id, revision:original.revision, categoryId:1, groupId:9, caption:"Keep this note"
  })));
});
it("reorders siblings with the keyboard controls and keeps their group", async () => {
  vi.mocked(orderLink).mockResolvedValue(undefined);
    categories.mockResolvedValue([{ ...inbox, groups: [{id: 7, name: "Papers"}] }]);
    list.mockResolvedValue(page(["First","Second"].map((title,i) => link({id:i+1,title,groupId:7}))));
    render(<App />);
    await screen.findByRole("button", {name:"Open First"});
    fireEvent.click(screen.getByRole("button",{name:"Organize"}));
    expect(screen.queryByRole("button",{name:"Move Second up"})).toBeNull();
    const up = screen.getByRole("button",{name:"Move Second"});
    up.focus();
    fireEvent.keyDown(up,{altKey:true,key:"ArrowUp"});
    await waitFor(() => expect(screen.getAllByRole("button",{name:/^Open (First|Second)$/})[0].getAttribute("aria-label")).toBe("Open Second"));
    expect(orderLink).toHaveBeenCalledWith(2,1,true);
    expect(document.activeElement).toBe(up);
    fireEvent.keyDown(up,{altKey:true,key:"ArrowUp"});
    expect(orderLink).toHaveBeenCalledTimes(1);

});
it("drops a sibling below another row and leaves their order intact on a failed write", async () => {
  vi.mocked(orderLink).mockRejectedValueOnce({code:"conflict",message:"Reload this group."}).mockResolvedValueOnce(undefined);
  list.mockResolvedValue(page(["First","Second"].map((title,i) => link({id:i+1,title}))));
  render(<App />);
  await screen.findByRole("button",{name:"Open First"});
  fireEvent.click(screen.getByRole("button",{name:"Organize"}));
  const source = screen.getByRole("button",{name:"Move First"});
  const target = screen.getByRole("button",{name:"Open Second"}).closest("li")!;
  const drop = () => {
    const dataTransfer = {setData:vi.fn(),effectAllowed:""};
    fireEvent.dragStart(source,{dataTransfer});
    fireEvent.dragOver(target,{dataTransfer});
    fireEvent.drop(target,{dataTransfer});
  };
  drop();
  await waitFor(() => expect(orderLink).toHaveBeenCalledWith(1,2,false));
  await screen.findByRole("alert");
  expect(screen.getAllByRole("button",{name:/^Open (First|Second)$/})[0].getAttribute("aria-label")).toBe("Open First");
  drop();
  await waitFor(() => expect(screen.getAllByRole("button",{name:/^Open (First|Second)$/})[0].getAttribute("aria-label")).toBe("Open Second"));
});

it("keeps the category disclosure separate from editing and retains focus on collapse", async () => {
  list.mockResolvedValue(page([link()]));
  render(<App />);
  await screen.findByRole("button", {name:"Open Kept title"});
  const disclosure = screen.getByRole("button", {name:"Inbox"});
  disclosure.focus();
  fireEvent.click(disclosure);
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(disclosure);
  expect(screen.queryByRole("button", {name:"Open Kept title"})).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", {name:"Edit category Inbox"}));
  expect(screen.getByRole("dialog", {name:"Edit category"})).toBeTruthy();
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
});

it("shows the connected account as information in the library heading", async () => {
  vi.mocked(driveBackup).mockResolvedValue({
    configured:true, requiresReconnect:false, profile:{name:"Preview Account"}, frequency:"weekly", lastBackup:null, warning:null,
  });
  render(<App />);
  const name = await screen.findByText("Preview Account");
  expect(name.closest(".library-heading")).toBe(screen.getByRole("heading", {name:"Linkme"}).parentElement);
  expect(name.closest("button, a, [tabindex]")).toBeNull();
});
