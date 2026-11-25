import { act, render } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { WaterField } from "./WaterField";

it("prepaints one glass reflection while the popup is unfocused", () => {
  const draw = vi.fn();
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => ({
    createImageData: () => ({ data: new Uint8ClampedArray(160 * 104 * 4) }),
    putImageData: draw,
  }) as unknown as CanvasRenderingContext2D);
  vi.stubGlobal("matchMedia", () => Object.assign(new EventTarget(), { matches: false }));
  document.documentElement.classList.add("native-acrylic");
  try {
    const view = render(<WaterField glass />);
    expect(draw).not.toHaveBeenCalled();
    window.dispatchEvent(new Event("linkme-popup-prepaint"));
    expect(draw).toHaveBeenCalledOnce();
    view.unmount();
    window.dispatchEvent(new Event("linkme-popup-prepaint"));
    expect(draw).toHaveBeenCalledOnce();
  } finally {
    document.documentElement.classList.remove("native-acrylic");
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});

it.each(["button", "a", "input"])("continues repainting over a stationary %s", (tag) => {
  vi.useFakeTimers();
  const upload = vi.fn(),
    allocate = vi.fn(() => ({ data: new Uint8ClampedArray(160 * 104 * 4) }));
  const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    () =>
      ({
        createImageData: allocate,
        putImageData: upload,
      }) as unknown as CanvasRenderingContext2D,
  );
  vi.stubGlobal("matchMedia", () =>
    Object.assign(new EventTarget(), { matches: false }),
  );
  try {
    const view = render(<WaterField />);
    act(() => vi.advanceTimersByTime(1000));
    expect(upload).toHaveBeenCalledTimes(1);
    const move = (el: Element) =>
      el.dispatchEvent(
        Object.assign(new Event("pointermove", { bubbles: true }), {
          clientX: 30,
          clientY: 40,
          pointerType: "mouse",
        }),
      );
    act(() => {
      move(view.container);
      vi.advanceTimersByTime(160);
    });
    expect(upload.mock.calls.length).toBeGreaterThan(1);
    const button = document.createElement(tag);
    view.container.append(button);
    act(() => {
      move(button);
      vi.advanceTimersByTime(160);
    });
    const onControl = upload.mock.calls.length;
    act(() => vi.advanceTimersByTime(2000));
    expect(upload.mock.calls.length).toBeGreaterThan(onControl);
    const beforeLeave = upload.mock.calls.length;
    act(() => {
      move(view.container);
      document.documentElement.dispatchEvent(new Event("pointerleave"));
      vi.advanceTimersByTime(1000);
    });
    expect(upload).toHaveBeenCalledTimes(beforeLeave);
    focus.mockReturnValue(false);
    act(() => window.dispatchEvent(new Event("blur")));
    const count = upload.mock.calls.length;
    act(() => vi.advanceTimersByTime(5000));
    expect(upload).toHaveBeenCalledTimes(count);
    focus.mockReturnValue(true);
    act(() => window.dispatchEvent(new Event("focus")));
    expect(upload.mock.calls.length).toBeGreaterThan(count);
    expect(allocate).toHaveBeenCalledOnce();
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});
