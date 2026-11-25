import { render, fireEvent, waitFor } from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { WebsiteIcon, faviconUrl, categorySymbol } from "./WebsiteIcon";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const fetchIcon = vi.mocked(invoke);
const raster = "data:image/png;base64,c3ludGhldGlj";
afterEach(() => {
  vi.restoreAllMocks();
  fetchIcon.mockReset();
});
it("requests only the HTTPS origin favicon without saved secrets", () => {
  expect(faviconUrl("http://example.com/private?token=secret#notes")).toBe(
    "https://example.com/favicon.ico",
  );
  for (const url of [
    "file:///a",
    "https://user:pass@example.com/a",
    "https://127.0.0.1/a",
    "https://printer.local/a",
    "https://example.com:8443/a",
  ]) {
    expect(faviconUrl(url)).toBeUndefined();
  }
});
it("uses category symbols offline, retries after reconnect and ignores remote image URLs", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  fetchIcon.mockResolvedValueOnce("https://external.invalid/unsafe-remote.png");
  fetchIcon.mockResolvedValueOnce(raster);
  const online = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  const v = render(
    <WebsiteIcon id={42} url="https://example.com/a" category="Reading" />,
  );
  expect(v.container.querySelector("img")).toBeNull();
  expect(fetchIcon).not.toHaveBeenCalled();
  expect(categorySymbol("Reading")).toBe("book");
  online.mockReturnValue(true);
  fireEvent(window, new Event("online"));
  await waitFor(() =>
    expect(fetchIcon).toHaveBeenCalledWith("site_icon", { id: 42 }),
  );
  expect(v.container.querySelector("img")).toBeNull();
  expect(v.container.querySelector("svg")).toBeTruthy();
  online.mockReturnValue(false);
  fireEvent(window, new Event("offline"));
  expect(v.container.querySelector("img")).toBeNull();
  online.mockReturnValue(true);
  fireEvent(window, new Event("online"));
  await waitFor(() =>
    expect(v.container.querySelector("img")?.getAttribute("src")).toBe(raster),
  );
});

it("deduplicates icons by host, preserves a cached image offline, and waits for focus", async () => {
  const focused = vi.spyOn(document, "hasFocus").mockReturnValue(false);
  const online = vi.spyOn(navigator, "onLine", "get").mockReturnValue(true);
  fetchIcon.mockResolvedValue(raster);
  const v = render(
    <>
      <WebsiteIcon id={43} url="https://example.org/a" category="Inbox" />
      <WebsiteIcon id={44} url="https://example.org/b" category="Inbox" />
    </>,
  );
  expect(fetchIcon).not.toHaveBeenCalled();
  focused.mockReturnValue(true);
  fireEvent(window, new Event("focus"));
  await waitFor(() =>
    expect(v.container.querySelectorAll("img")).toHaveLength(2),
  );
  expect(fetchIcon).toHaveBeenCalledTimes(1);
  const img = v.container.querySelector("img")!;
  online.mockReturnValue(false);
  fireEvent(window, new Event("offline"));
  expect(v.container.querySelector("img")).toBe(img);
  online.mockReturnValue(true);
  fireEvent(window, new Event("online"));
  expect(v.container.querySelector("img")).toBe(img);
});
