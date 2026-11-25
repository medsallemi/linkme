import { expect, it } from "vitest";
import { RippleField, paintRipple } from "./rippleField";
it("propagates a local impulse and dissipates its energy", () => {
  const field = new RippleField(32, 24);
  field.height.fill(0);
  field.disturb(0.5, 0.5, 0.12);
  for (let i = 0; i < 18; i++) field.step();
  expect(Math.abs(field.height[12 * 32 + 23])).toBeGreaterThan(0.001);
  for (let i = 0; i < 800; i++) field.step();
  expect(Math.max(...field.height.map(Math.abs))).toBeLessThan(0.001);
});
it("keeps the popup lightly tinted and translucent while the dark library mixes its own colors", () => {
  const field = new RippleField(32, 24),
    data = new Uint8ClampedArray(32 * 24 * 4);
  paintRipple(field, data, true, true);
  for (let i = 0; i < data.length; i += 4) {
    expect(data[i + 1]).toBeGreaterThan(data[i]);
    expect(data[i + 3]).toBeGreaterThanOrEqual(8);
    expect(data[i + 3]).toBeLessThan(35);
  }
  paintRipple(field, data, false, true);
  expect(new Set(data.filter((_, i) => i % 4 === 1)).size).toBeGreaterThan(10);
  expect(Math.max(...data.filter((_, i) => i % 4 !== 3))).toBeLessThan(64);
  expect(data[3]).toBe(255);
});

it("advances the ripple more gently at the ambient time step", () => {
  const pulse = (dt: number) => {
    const field = new RippleField(160, 104);
    field.height.fill(0);
    field.disturb(0.5, 0.5, 0.05);
    field.step(dt);
    return Math.max(...field.height);
  };
  expect(pulse(0.55)).toBeLessThan(pulse(1) * 0.6);
});
