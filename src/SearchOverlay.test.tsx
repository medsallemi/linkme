import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listLinks } from "./links";
import type { Link } from "./links";
import { SearchOverlay } from "./SearchOverlay";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./links", async (actual) => ({
  ...(await actual<typeof import("./links")>()),
  listLinks: vi.fn(),
}));

const link = (id: number, title: string): Link => ({
  id,
  title,
  url: `https://example.com/${id}`,
  revision: 0,
  subtitle: "",
  caption: "",
  categoryId: 1,
  categoryName: "Inbox",
});

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(listLinks).mockResolvedValue({
    links: [link(1, "First"), link(2, "Second")],
    next: null,
  });
  vi.mocked(invoke).mockResolvedValue(undefined);
});

afterEach(() => {
  delete document.documentElement.dataset.popupGeneration;
});

it("reports the focused search shell as soon as its material is ready, even while results are pending", () => {
  vi.mocked(listLinks).mockReturnValue(new Promise(() => {}));
  let focusedOnReady: Element | null = null;
  vi.mocked(invoke).mockImplementation(async () => {
    focusedOnReady = document.activeElement;
  });
  render(<SearchOverlay />);
  const input = screen.getByRole("combobox", { name: "Search saved links" });
  expect(invoke).not.toHaveBeenCalled();
  document.documentElement.dataset.popupGeneration = "7";
  window.dispatchEvent(new Event("linkme-popup-material-ready"));
  window.dispatchEvent(new Event("linkme-popup-material-ready"));
  expect(invoke).toHaveBeenCalledExactlyOnceWith("popup_ready", {
    generation: 7,
  });
  expect(focusedOnReady).toBe(input);
});

it("keeps input focus and opens the keyboard-selected saved ID", async () => {
  render(<SearchOverlay />);
  const input = screen.getByRole("combobox", { name: "Search saved links" });
  await screen.findByRole("row", { name: /Second/ });
  expect(document.activeElement).toBe(input);
  fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(
    screen.getByRole("row", { name: /Second/ }).getAttribute("aria-selected"),
  ).toBe("true");
  fireEvent.keyDown(input, { key: "Enter" });
  expect(invoke).toHaveBeenCalledWith("open_search_result", { id: 2 });
});

it("does not open a result from an older query and Escape clears before closing", async () => {
  render(<SearchOverlay />);
  const input = screen.getByRole("combobox");
  await screen.findByRole("row", { name: /First/ });
  fireEvent.change(input, { target: { value: "different" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(vi.mocked(invoke).mock.calls.filter(([name]) => name !== "resize_search_panel")).toHaveLength(0);
  fireEvent.keyDown(input, { key: "Escape", isComposing: true });
  expect((input as HTMLInputElement).value).toBe("different");
  fireEvent.keyDown(input, { key: "Escape" });
  expect((input as HTMLInputElement).value).toBe("");
  expect(vi.mocked(invoke).mock.calls.filter(([name]) => name !== "resize_search_panel")).toHaveLength(0);
  fireEvent.keyDown(document.body, { key: "Escape" });
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("dismiss_capture_panel"),
  );
});

it("keeps the same selection if the native browser open fails and permits retry", async () => {
  render(<SearchOverlay />);
  const input = screen.getByRole("combobox");
  await screen.findByRole("row", { name: /First/ });
  vi.mocked(invoke).mockRejectedValueOnce({
    code: "not_found",
    message: "This link no longer exists.",
  });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "This link no longer exists.",
  );
  expect(document.activeElement).toBe(input);
  fireEvent.keyDown(input, { key: "Enter" });
  expect(invoke).toHaveBeenLastCalledWith("open_search_result", { id: 1 });
  expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "open_search_result")).toHaveLength(2);
});

it("closes the frameless overlay through its own close button", async () => {
  render(<SearchOverlay />);
  await screen.findByRole("row", { name: /First/ });
  fireEvent.click(screen.getByRole("button", { name: "Close search" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("dismiss_capture_panel"),
  );
  expect(invoke).not.toHaveBeenCalledWith(
    "open_search_result",
    expect.anything(),
  );
});

it("copies or shares the requested row without opening its URL", async () => {
  render(<SearchOverlay />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Copy URL for Second" }),
  );
  await screen.findByText("Link copied.");
  fireEvent.click(screen.getByRole("button", { name: "Share First" }));
  await screen.findByText("Email draft opened.");
  expect(invoke).toHaveBeenCalledWith("copy_link", { id: 2 });
  expect(invoke).toHaveBeenCalledWith("share_link_email", { id: 1 });
  expect(invoke).not.toHaveBeenCalledWith(
    "open_search_result",
    expect.anything(),
  );
});

it("shares the selected URL with the OS when available and treats cancellation as ordinary dismissal", async () => {
  const share = vi
    .fn()
    .mockRejectedValue(new DOMException("Canceled", "AbortError"));
  const property = Object.getOwnPropertyDescriptor(navigator, "share");
  Object.defineProperty(navigator, "share", {
    value: share,
    configurable: true,
  });
  try {
    render(<SearchOverlay />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Share Second" }),
    );
    await waitFor(() =>
      expect(share).toHaveBeenCalledWith({
        title: "Second",
        url: "https://example.com/2",
      }),
    );
    expect(vi.mocked(invoke).mock.calls.filter(([name]) => name !== "resize_search_panel")).toHaveLength(0);
    expect(screen.queryByRole("alert")).toBeNull();
  } finally {
    if (property) Object.defineProperty(navigator, "share", property);
    else Reflect.deleteProperty(navigator, "share");
  }
});

it("includes the group beside the category in a search result", async () => {
  vi.mocked(listLinks).mockResolvedValue({links:[{...link(1,"Research"),groupId:8,groupName:"Papers"}], next:null});
  render(<SearchOverlay />);
  expect((await screen.findByText("Papers")).closest('[role="row"]')?.textContent).toContain("Inbox");
});

it("pages results without scrolling and lets ArrowDown cross a page boundary", async () => {
  vi.mocked(listLinks).mockResolvedValue({
    links: [1, 2, 3, 4, 5].map(id => link(id, 'Article ' + id)), next: null,
  });
  render(<SearchOverlay />);
  const query = screen.getByRole("combobox");
  await screen.findByRole("button", {name:"Open Article 1"});
  expect(invoke).not.toHaveBeenCalledWith("resize_search_panel", {results:5});
  expect(screen.queryByRole("button", {name:"Open Article 4"})).toBeNull();
  for (let i = 0; i < 3; i++) fireEvent.keyDown(query,{key:"ArrowDown"});
  expect(screen.getByRole("button", {name:"Open Article 4"})).toBeTruthy();
  expect(query.getAttribute("aria-activedescendant")).toBe("search-result-4");
  fireEvent.click(screen.getByRole("button", {name:"Previous results"}));
  expect(screen.getByRole("button", {name:"Open Article 1"})).toBeTruthy();
});
