import { invoke } from "@tauri-apps/api/core";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { PreviewLink } from "./PreviewLink";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it("waits for deliberate hover, dismisses with Escape, and keeps normal opening", async () => {
  vi.useFakeTimers();
  const onOpen = vi.fn();
  vi.mocked(invoke).mockResolvedValue({ title: "Page title", description: "Page summary" });
  const props = { id: 7, url: "https://example.org/article", label: "Example", disabled: false, onOpen };
  const view = render(<PreviewLink {...props} enabled={false} />);
  const link = screen.getByRole("button", { name: "Open Example" });
  try {
    fireEvent.pointerEnter(link);
    await act(() => vi.advanceTimersByTimeAsync(500));
    expect(invoke).not.toHaveBeenCalled();
    view.rerender(<PreviewLink {...props} enabled />);
    fireEvent.pointerEnter(link);
    await act(() => vi.advanceTimersByTimeAsync(200));
    fireEvent.pointerLeave(link);
    await act(() => vi.advanceTimersByTimeAsync(500));
    expect(invoke).not.toHaveBeenCalled();
    fireEvent.pointerEnter(link);
    await act(() => vi.advanceTimersByTimeAsync(450));
    expect(invoke).toHaveBeenCalledExactlyOnceWith("preview_link", { id: 7 });
    expect(screen.getByRole("tooltip").textContent).toContain("Page summary");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.click(link);
    expect(onOpen).toHaveBeenCalledOnce();
  } finally {
    vi.useRealTimers();
  }
});
it("previews a query-bearing URL from its displayed address and gives a saved card on failure", async () => {
  vi.useFakeTimers();
  vi.mocked(invoke).mockRejectedValue(new Error("Private page"));
  const url = "https://www.youtube.com/watch?v=abcdefghijk&t=273s";
  try {
    render(<PreviewLink id={8} url={url} label="My video" displayUrl enabled disabled={false} onOpen={() => {}} />);
    fireEvent.pointerEnter(screen.getByText(url));
    await act(() => vi.advanceTimersByTimeAsync(451));
    expect(invoke).toHaveBeenCalledWith("preview_link", {id:8});
    expect(screen.getByRole("tooltip").textContent).toContain("My video");
    expect(screen.getByRole("tooltip").textContent).toContain("Website preview unavailable");
  } finally { vi.useRealTimers(); }
});

it("shows a thumbnail first and preserves the title when image decoding fails", async () => {
  vi.useFakeTimers();
  vi.mocked(invoke).mockResolvedValue({
    title: "Video title", description: "Channel", image: "data:image/png;base64,AAAA",
  });
  try {
    render(<PreviewLink id={9} url="https://youtu.be/abcdefghijk" label="Video" enabled disabled={false} onOpen={() => {}} />);
    fireEvent.pointerEnter(screen.getByRole("button", { name: "Open Video" }));
    await act(() => vi.advanceTimersByTimeAsync(450));
    const card = screen.getByRole("tooltip");
    const thumbnail = card.querySelector("img")!;
    expect(card.firstElementChild).toBe(thumbnail);
    expect(thumbnail.alt).toBe("");
    fireEvent.error(thumbnail);
    expect(card.querySelector("img")).toBeNull();
    expect(card.textContent).toContain("Video title");
  } finally {
    vi.useRealTimers();
  }
});
