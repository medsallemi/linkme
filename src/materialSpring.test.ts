import { it, expect } from "vitest";
import { advanceSpring } from "./materialSpring";
it("converges without oscillation and converges equally at different refresh rates", () => {
  const sample = (hz: number) => {
    const axis = { position: 0, velocity: 0 };
    for (let i = 0; i < hz; i++) {
      advanceSpring(axis, 6, 1 / hz);
      expect(axis.position).toBeLessThanOrEqual(6);
    }
    return axis.position;
  };
  expect(sample(60)).toBeCloseTo(sample(144), 9);
  expect(sample(60)).toBeCloseTo(6, 2);
});
it("retains momentum when the pointer reverses, then settles", () => {
  const axis = { position: 0, velocity: 0 };
  advanceSpring(axis, 6, 0.08);
  const before = axis.position;
  advanceSpring(axis, -6, 0.001);
  expect(axis.position).toBeGreaterThan(before);
  advanceSpring(axis, -6, 2);
  expect(axis.position).toBeCloseTo(-6, 5);
});
