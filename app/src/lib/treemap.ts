// Squarified treemap layout (Bruls, Huizing & van Wijk, 2000).

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Placed<T> extends Rect {
  item: T;
}

/** Worst aspect ratio of a row with the given total area and extremes. */
function worst(sum: number, min: number, max: number, side: number): number {
  const s2 = side * side;
  const sum2 = sum * sum;
  return Math.max((s2 * max) / sum2, sum2 / (s2 * min));
}

/**
 * Lays out `items` (largest first gives the best result) inside `rect`, so
 * each rectangle's area is proportional to its value. Items with a value of
 * zero or less are left out.
 */
export function squarify<T>(items: ReadonlyArray<{ value: number; item: T }>, rect: Rect): Placed<T>[] {
  const out: Placed<T>[] = [];
  const positive = items.filter((i) => i.value > 0 && Number.isFinite(i.value));
  const total = positive.reduce((s, i) => s + i.value, 0);
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) return out;

  const scale = (rect.w * rect.h) / total;
  const areas = positive.map((i) => ({ area: i.value * scale, item: i.item }));
  let { x, y, w, h } = rect;
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(w, h);
    const first = areas[i]!;
    let sum = first.area;
    let min = first.area;
    let max = first.area;
    let best = worst(sum, min, max, side);
    let j = i + 1;
    while (j < areas.length) {
      const a = areas[j]!.area;
      const next = worst(sum + a, Math.min(min, a), Math.max(max, a), side);
      if (next > best) break;
      sum += a;
      min = Math.min(min, a);
      max = Math.max(max, a);
      best = next;
      j += 1;
    }
    const thickness = sum / side;
    if (w >= h) {
      // Column along the left edge.
      let cy = y;
      for (let k = i; k < j; k++) {
        const r = areas[k]!;
        const rh = r.area / thickness;
        out.push({ x, y: cy, w: thickness, h: rh, item: r.item });
        cy += rh;
      }
      x += thickness;
      w -= thickness;
    } else {
      // Row along the top edge.
      let cx = x;
      for (let k = i; k < j; k++) {
        const r = areas[k]!;
        const rw = r.area / thickness;
        out.push({ x: cx, y, w: rw, h: thickness, item: r.item });
        cx += rw;
      }
      y += thickness;
      h -= thickness;
    }
    i = j;
  }
  return out;
}
