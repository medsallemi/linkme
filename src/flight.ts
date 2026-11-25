export interface Scene {
  start: [number, number];
  endX: number;
  category: string;
  host: string;
  highlight: number;
  transparency: number;
  glass: boolean;
  sourceSize?: [number, number];
}

export const travelMs = 680;
export const sceneMs = travelMs + 1160;
const staticSceneMs = 1680;

// Sample one cubic in client pixels. CSS transforms then run independently of JavaScript.
export function flightFrames(
  x: number,
  y: number,
  endX: number,
  endY: number,
  width: number,
  sourceSize?: [number, number],
): Keyframe[] {
  const rise = Math.max(0, y - endY);
  const bow = Math.min(105, width * 0.2, rise * 0.38) * (x >= endX ? 1 : -1);
  return Array.from({ length: 25 }, (_, index) => {
    const t = index / 24,
      u = 1 - t;
    const cx =
      u * u * u * x +
      3 * u * u * t * (x + bow) +
      3 * u * t * t * (endX + bow * 0.52) +
      t * t * t * endX;
    const cy =
      u * u * u * y +
      3 * u * u * t * (y - rise * 0.1) +
      3 * u * t * t * (endY + rise * 0.07) +
      t * t * t * endY;
    const scale = 1 + 0.1 * Math.sin(t * Math.PI) - 0.7 * t * t;
    const stretch = 1 + 0.8 * Math.sin(t * Math.PI) * (1 - t);
    const gather = Math.pow(1 - t, 3);
    const sx = sourceSize ? 0.025 + 0.975 * gather : scale / stretch;
    const sy = sourceSize
      ? (0.018 + 0.982 * Math.pow(1 - t, 1.7)) * stretch
      : scale * stretch;
    const tilt = -Math.sign(bow) * 14 * Math.sin(t * Math.PI);
    return {
      offset: t,
      transform: `translate(${cx}px, ${cy}px) translate(-50%, -50%) rotate(${tilt}deg) scale(${sx}, ${sy})`,
      opacity: Math.min(1, u * 7),
    };
  });
}

export function playScene(
  scene: Scene,
  root: HTMLElement,
  chip: HTMLElement,
  island: HTMLElement,
  reduced: MediaQueryList,
) {
  const width = root.clientWidth,
    height = root.clientHeight;
  const endX = scene.endX * width,
    endY = Math.min(34, height / 2);
  root.dataset.captureHighlight = String(scene.highlight);
  root.dataset.glass = String(scene.glass);
  root.style.setProperty(
    "--clear",
    `${Math.min(70, scene.transparency * 2.8)}%`,
  );
  island.style.left = `${endX}px`;
  island.style.top = `${endY}px`;
  chip.querySelector("bdi")!.textContent = scene.host;
  island.querySelector("bdi")!.textContent = scene.category;
  if (scene.sourceSize) {
    chip.style.width = `${scene.sourceSize[0]}px`;
    chip.style.height = `${scene.sourceSize[1]}px`;
  }
  const animations: Animation[] = [];
  const stopMotion = () => {
    for (const a of animations.splice(0)) a.cancel();
    root.dataset.phase = "static";
  };
  root.style.setProperty("--arrival", travelMs + "ms");
  root.dataset.phase = "static";
  const animated = !reduced.matches && typeof chip.animate === "function";
  if (animated) {
    root.dataset.phase = "flying";
    animations.push(
      chip.animate(
        flightFrames(
          scene.start[0] * width,
          scene.start[1] * height,
          endX,
          endY,
          width,
          scene.sourceSize,
        ),
        {
          duration: travelMs,
          delay: 0,
          easing: "cubic-bezier(.42,.06,.32,1)",
          fill: "both",
        },
      ),
    );
  }
  const onPreference = () => {
    if (reduced.matches) stopMotion();
  };
  reduced.addEventListener("change", onPreference);
  const cleanup = () => {
    clearTimeout(timer);
    reduced.removeEventListener("change", onPreference);
    stopMotion();
    delete root.dataset.phase;
    chip.querySelector("bdi")!.textContent = "";
    island.querySelector("bdi")!.textContent = "";
  };
  const timer = setTimeout(cleanup, animated ? sceneMs : staticSceneMs);
  return cleanup;
}
