import { useEffect, useRef } from "react";
import { RippleField, paintRipple } from "./rippleField";
import { advanceSpring } from "./materialSpring";

export function WaterField({ glass = false }: { glass?: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current!,
      ctx = canvas.getContext("2d", { alpha: glass });
    if (!ctx) return;
    const field = new RippleField(canvas.width, canvas.height);
    const pixels = ctx.createImageData(canvas.width, canvas.height);
    const still = matchMedia("(prefers-reduced-motion: reduce)"),
      solid = matchMedia(
        "(prefers-reduced-transparency: reduce), (forced-colors: active)",
      ),
      dark = matchMedia("(prefers-color-scheme: dark)");
    const x = { position: 0.5, velocity: 0 },
      y = { ...x };
    let targetX = 0.5,
      targetY = 0.5,
      energy = 0,
      phase = Math.random() * 6.28;
    let frame = 0,
      lastPaint = 0;
    function visible() {
      return (
        !document.hidden &&
        !solid.matches &&
        (!glass ||
          document.documentElement.classList.contains("native-acrylic"))
      );
    }
    function draw() {
      paintRipple(field, pixels.data, glass, dark.matches);
      ctx!.putImageData(pixels, 0, 0);
    }
    function stop() {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      energy = 0;
      lastPaint = 0;
    }
    function tick(now: number) {
      frame = 0;
      if (
        !visible() ||
        !document.hasFocus() ||
        still.matches
      ) {
        stop();
        return;
      }
      if (now - lastPaint >= 30) {
        lastPaint = now;
        phase += 0.006;
        advanceSpring(x, targetX, 1 / 30);
        advanceSpring(y, targetY, 1 / 30);
        field.disturb(
          0.45 + 0.35 * Math.sin(phase * 0.71),
          0.47 + 0.33 * Math.cos(phase * 0.93),
          0.009 * Math.cos(phase * 1.3),
        );
        field.disturb(x.position, y.position, energy);
        energy *= 0.77;
        field.step(0.55);
        draw();
      }
      frame = requestAnimationFrame(tick);
    }
    function sync() {
      stop();
      if (visible() && document.hasFocus()) {
        draw();
        if (!still.matches && document.documentElement.matches(":hover")) {
          frame = requestAnimationFrame(tick);
        }
      }
    }
    function move(e: PointerEvent) {
      if (
        !visible() ||
        !document.hasFocus() ||
        still.matches ||
        e.pointerType === "touch"
      )
        return;
      const a = e.clientX / Math.max(1, innerWidth),
        b = e.clientY / Math.max(1, innerHeight);
      energy = Math.min(
        0.055,
        energy + Math.hypot(a - targetX, b - targetY) * 0.25,
      );
      targetX = a;
      targetY = b;
      if (!frame) {
        frame = requestAnimationFrame(tick);
      }
    }
    window.addEventListener("pointermove", move, { passive: true, capture: true });
    document.documentElement.addEventListener("pointerenter", move);
    document.documentElement.addEventListener("pointerleave", stop);
    document.addEventListener("visibilitychange", sync);
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    for (const q of [still, solid, dark]) q.addEventListener("change", sync);
    const nativeChange = glass ? new MutationObserver(sync) : undefined;
    const prepaint = () => {
      if (!solid.matches && document.documentElement.classList.contains("native-acrylic")) draw();
    };
    if (glass) window.addEventListener("linkme-popup-prepaint", prepaint);
    nativeChange?.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    sync();
    return () => {
      nativeChange?.disconnect();
      if (glass) window.removeEventListener("linkme-popup-prepaint", prepaint);
      stop();
      document.documentElement.removeEventListener("pointerleave", stop);
      window.removeEventListener("pointermove", move, true);
      document.documentElement.removeEventListener("pointerenter", move);
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
      for (const q of [still, solid, dark])
        q.removeEventListener("change", sync);
    };
  }, [glass]);
  return (
    <canvas
      className="water-field"
      width={160}
      height={104}
      ref={ref}
      aria-hidden="true"
    />
  );
}
