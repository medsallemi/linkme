import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { flightFrames, playScene, sceneMs, type Scene } from "./flight";

const scene: Scene = {
  start: [0.7, 0.8],
  endX: 0.48,
  host: "example.com",
  category: "Books <2026>",
  highlight: 5,
  transparency: 25,
  glass: true,
};
function setup(reduce = false) {
  const root = document.createElement("main"),
    chip = document.createElement("div"),
    island = document.createElement("div");
  root.append(chip, island);
  chip.append(document.createElement("bdi"));
  island.append(document.createElement("bdi"));
  Object.defineProperties(root, {
    clientWidth: { value: 500 },
    clientHeight: { value: 400 },
  });
  const cancel = vi.fn();
  chip.animate = vi.fn(() => ({ cancel }) as unknown as Animation);
  const media = Object.assign(new EventTarget(), {
    matches: reduce,
  }) as MediaQueryList;
  return {
    root,
    chip,
    island,
    cancel,
    media,
    start: (s = scene) => playScene(s, root, chip, island, media),
  };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
it("carries the chip from its measured position to the island on a bowed path", () => {
  const frames = flightFrames(200, 320, 200, 34, 480);
  expect(frames[0].transform).toContain("translate(200px, 320px)");
  expect(frames.at(-1)?.transform).toContain("translate(200px, 34px)");
  expect(frames[12].transform).not.toContain("translate(200px");
  expect(frames.at(-1)?.opacity).toBe(0);
});
it("renders the committed category as literal text and expires without an animation event", () => {
  const v = setup();
  v.start();
  expect(v.root.dataset.phase).toBe("flying");
  expect(v.island.textContent).toBe("Books <2026>");
  expect(v.chip.textContent).toBe("example.com");
  expect(v.root.style.getPropertyValue("--clear")).toBe("70%");
  vi.advanceTimersByTime(sceneMs);
  expect(v.root.dataset.phase).toBeUndefined();
  expect(v.cancel).toHaveBeenCalledOnce();
  expect(v.island.textContent).toBe("");
});
it("gives the traveler 680 ms and keeps the island alive through the confirmation", () => {
  const v = setup();
  v.start();
  expect(v.chip.animate).toHaveBeenCalledWith(
    expect.any(Array),
    expect.objectContaining({ duration: 680 }),
  );
  expect(v.root.style.getPropertyValue("--arrival")).toBe("680ms");
  vi.advanceTimersByTime(1839);
  expect(v.island.textContent).toBe(scene.category);
  vi.advanceTimersByTime(1);
  expect(v.root.dataset.phase).toBeUndefined();
});
it("uses static category feedback with reduced motion and a solid material when requested", () => {
  const v = setup(true);
  v.start({ ...scene, glass: false });
  expect(v.root.dataset.phase).toBe("static");
  expect(v.root.dataset.glass).toBe("false");
  expect(v.chip.animate).not.toHaveBeenCalled();
  expect(v.island.textContent).toBe(scene.category);
  vi.advanceTimersByTime(1680);
  expect(v.root.dataset.phase).toBeUndefined();
});
it("interrupts travel on a preference change and cancels an old expiry before replay", () => {
  const v = setup();
  const stop = v.start();
  vi.advanceTimersByTime(580);
  Object.defineProperty(v.media, "matches", { value: true });
  v.media.dispatchEvent(new Event("change"));
  expect(v.cancel).toHaveBeenCalledOnce();
  expect(v.root.dataset.phase).toBe("static");
  stop();
  v.start({ ...scene, category: "Inbox" });
  vi.advanceTimersByTime(sceneMs - 580);
  expect(v.island.textContent).toBe("Inbox");
  vi.advanceTimersByTime(580);
  expect(v.island.textContent).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});

it("gathers the panel volume into an elongated drop before arrival", () => {
  const frames = flightFrames(210, 330, 200, 34, 488, [440, 358]);
  expect(frames[0].transform).toContain("scale(1, 1)");
  expect(frames[12].transform).toContain("scale(");
  expect(frames.at(-1)?.transform).toContain("scale(0.025, 0.018)");
});
