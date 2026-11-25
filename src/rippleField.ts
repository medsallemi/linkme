/** Damped shallow-water height field. The Laplacian transfers velocity to adjacent cells. */
export class RippleField {
  readonly height: Float32Array;
  private readonly velocity: Float32Array;

  constructor(
    readonly columns: number,
    readonly rows: number,
  ) {
    this.height = new Float32Array(columns * rows);
    this.velocity = new Float32Array(this.height.length);
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < columns; x++) {
        const u = (x * 80) / columns,
          v = (y * 52) / rows;
        this.height[y * columns + x] =
          0.48 * Math.sin(u * 0.085 + 1.6 * Math.cos(v * 0.09)) +
          0.36 * Math.cos(v * 0.13 - u * 0.055);
      }
    }
  }

  disturb(x: number, y: number, impulse: number) {
    const cx = x * (this.columns - 1),
      cy = y * (this.rows - 1);
    const radius = Math.max(6, this.columns * 0.075);
    for (
      let j = Math.max(1, Math.floor(cy - radius));
      j <= Math.min(this.rows - 2, cy + radius);
      j++
    ) {
      for (
        let i = Math.max(1, Math.floor(cx - radius));
        i <= Math.min(this.columns - 2, cx + radius);
        i++
      ) {
        const falloff = Math.max(
          0,
          1 - ((i - cx) ** 2 + (j - cy) ** 2) / (radius * radius),
        );
        this.velocity[j * this.columns + i] += impulse * falloff * falloff;
      }
    }
  }

  step(dt = 1) {
    const damping = 0.977 ** dt;
    const w = this.columns,
      h = this.height;
    for (let y = 0; y < this.rows; y++) {
      for (let x = 0; x < w; x++) {
        const i = y * w + x,
          v = h[i];
        const neighbors =
          h[x ? i - 1 : i] +
          h[x < w - 1 ? i + 1 : i] +
          h[y ? i - w : i] +
          h[y < this.rows - 1 ? i + w : i];
        this.velocity[i] =
          damping * this.velocity[i] +
          dt * (0.34 * (neighbors - 4 * v) - 0.0012 * v);
      }
    }
    for (let i = 0; i < h.length; i++) h[i] += dt * this.velocity[i];
  }
}

// Normals alter the mix of the two pigments; a narrow reflection travels along the folds.
export function paintRipple(
  field: RippleField,
  pixels: Uint8ClampedArray,
  glass: boolean,
  dark: boolean,
) {
  const { columns: w, height: h } = field;
  for (let i = 0; i < h.length; i++) {
    const slope =
      h[i] - h[i % w ? i - 1 : i] + 0.7 * (h[i] - h[Math.max(0, i - w)]);
    const mix = Math.max(0, Math.min(1, 0.49 + h[i] * 0.55 + slope * 1.8));
    const ridge = 1 / (1 + 180 * (h[i] - 0.13 + slope * 1.5) ** 2);
    const n = i * 4;
    if (glass) {
      // Continuous pearl-to-mint reflection. Acrylic still supplies the underlying color.
      const pearl = Math.max(0, Math.min(1, 0.5 + 2.5 * slope));
      pixels[n] = 63 + 154 * pearl;
      pixels[n + 1] = 170 + 60 * pearl + 15 * mix;
      pixels[n + 2] = 207 + 43 * pearl - 35 * mix;
      pixels[n + 3] = (dark ? 12 : 8) + 12 * mix + 10 * ridge;
    } else {
      pixels[n] = dark ? 8 + 3 * mix + 9 * ridge : 159 + 46 * mix + 24 * ridge;
      pixels[n + 1] = dark
        ? 16 + 40 * mix + 6 * ridge
        : 198 + 39 * mix + 10 * ridge;
      pixels[n + 2] = dark
        ? 40 + 7 * mix + 11 * ridge
        : 232 - 29 * mix + 16 * ridge;
      pixels[n + 3] = 255;
    }
  }
}
