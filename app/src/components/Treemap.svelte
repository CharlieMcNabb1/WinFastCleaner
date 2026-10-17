<script lang="ts">
  import { onMount } from "svelte";

  import type { Backend } from "../lib/backend.ts";
  import { CATEGORY_LABELS, type Category, category, formatPercent, formatSize } from "../lib/format.ts";
  import { type Rect, squarify } from "../lib/treemap.ts";
  import type { TreemapNode } from "../lib/types.ts";

  let {
    api,
    scanId,
    nodeId,
    onOpen,
  }: {
    api: Backend;
    scanId: number;
    nodeId: number;
    onOpen: (id: number) => void;
  } = $props();

  // Hues deliberately avoid green, yellow, and red, which are reserved for
  // the cleaner's safety tiers.
  const COLORS: Record<Category, string> = {
    folder: "#8ea2d8",
    video: "#5c6bd6",
    audio: "#9a5fd6",
    image: "#c25ac9",
    archive: "#2f8fcf",
    program: "#3f6fe0",
    diskImage: "#1f8aa3",
    document: "#6b778c",
    code: "#7c6fe0",
    other: "#9aa5b8",
  };
  const OTHER_COLOR = "#b9c0cc";
  const HEADER = 18;

  interface Hit extends Rect {
    id: number | null;
    openId: number | null;
    name: string;
    size: number;
    label: string;
  }

  let container: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let data = $state<TreemapNode | null>(null);
  let error = $state<string | null>(null);
  let width = $state(0);
  let height = $state(0);
  let dark = $state(false);
  let hover = $state<{ x: number; y: number; hit: Hit } | null>(null);
  let hits: Hit[] = [];

  $effect(() => {
    const id = nodeId;
    const scan = scanId;
    error = null;
    api
      .treemap(scan, id, 2, 120)
      .then((d) => {
        if (id === nodeId && scan === scanId) data = d;
      })
      .catch((e) => {
        error = String(e);
      });
  });

  onMount(() => {
    const ro = new ResizeObserver(([entry]) => {
      if (!entry) return;
      width = Math.floor(entry.contentRect.width);
      height = Math.floor(entry.contentRect.height);
    });
    ro.observe(container);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    dark = mq.matches;
    const onTheme = (e: MediaQueryListEvent) => (dark = e.matches);
    mq.addEventListener("change", onTheme);
    return () => {
      ro.disconnect();
      mq.removeEventListener("change", onTheme);
    };
  });

  $effect(() => {
    draw(data, width, height, dark);
  });

  function textColor(hex: string): string {
    const n = parseInt(hex.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return 0.299 * r + 0.587 * g + 0.114 * b > 150 ? "#111" : "#fff";
  }

  function fit(ctx: CanvasRenderingContext2D, text: string, max: number): string | null {
    if (max < 24) return null;
    if (ctx.measureText(text).width <= max) return text;
    let lo = 0;
    let hi = text.length;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (ctx.measureText(text.slice(0, mid) + "…").width <= max) lo = mid;
      else hi = mid - 1;
    }
    return lo > 1 ? text.slice(0, lo) + "…" : null;
  }

  function block(ctx: CanvasRenderingContext2D, r: Rect, fill: string, label: string, sub: string) {
    ctx.fillStyle = fill;
    ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.strokeStyle = dark ? "#111319" : "#ffffff";
    ctx.lineWidth = 1;
    ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
    if (r.h < 16) return;
    ctx.fillStyle = textColor(fill);
    const t = fit(ctx, label, r.w - 8);
    if (t) ctx.fillText(t, r.x + 4, r.y + 13);
    if (r.h >= 32) {
      const s = fit(ctx, sub, r.w - 8);
      if (s) {
        ctx.globalAlpha = 0.8;
        ctx.fillText(s, r.x + 4, r.y + 27);
        ctx.globalAlpha = 1;
      }
    }
  }

  function items(node: TreemapNode) {
    const list: { value: number; item: TreemapNode | null }[] = node.children.map((c) => ({ value: c.size, item: c }));
    if (node.otherSize > 0) list.push({ value: node.otherSize, item: null });
    return list;
  }

  function draw(d: TreemapNode | null, w: number, h: number, isDark: boolean) {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, w * dpr);
    canvas.height = Math.max(1, h * dpr);
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.font = "12px 'Segoe UI', system-ui, sans-serif";
    hits = [];
    if (!d || d.size === 0 || w < 10 || h < 10) return;

    for (const p of squarify(items(d), { x: 0, y: 0, w, h })) {
      const c = p.item;
      if (!c) {
        const label = `${d.otherCount.toLocaleString()} smaller items`;
        block(ctx, p, OTHER_COLOR, label, formatSize(d.otherSize));
        hits.push({ ...p, id: null, openId: null, name: label, size: d.otherSize, label: "Grouped" });
        continue;
      }
      const cat = category(c.name, c.isDir);
      const sub = `${formatSize(c.size)} · ${formatPercent(c.size, d.size)}`;
      if (c.isDir && (c.children.length > 0 || c.otherSize > 0) && p.w > 48 && p.h > HEADER + 14) {
        // Folder frame with a header, contents laid out inside.
        ctx.fillStyle = isDark ? "#2a3045" : "#dfe5f5";
        ctx.fillRect(p.x, p.y, p.w, p.h);
        ctx.strokeStyle = isDark ? "#111319" : "#ffffff";
        ctx.strokeRect(p.x + 0.5, p.y + 0.5, p.w - 1, p.h - 1);
        ctx.fillStyle = isDark ? "#e6e9f1" : "#1a1e29";
        const t = fit(ctx, `${c.name}  ${formatSize(c.size)}`, p.w - 8);
        if (t) ctx.fillText(t, p.x + 4, p.y + 13);
        hits.push({ ...p, id: c.id, openId: c.id, name: c.name, size: c.size, label: "Folder" });
        const inner = { x: p.x + 2, y: p.y + HEADER, w: p.w - 4, h: p.h - HEADER - 2 };
        for (const q of squarify(items(c), inner)) {
          const g = q.item;
          if (!g) {
            block(ctx, q, OTHER_COLOR, `${c.otherCount} more`, formatSize(c.otherSize));
            hits.push({ ...q, id: null, openId: c.id, name: `${c.otherCount} smaller items`, size: c.otherSize, label: "Grouped" });
            continue;
          }
          const gc = category(g.name, g.isDir);
          block(ctx, q, COLORS[gc], g.name, formatSize(g.size));
          hits.push({
            ...q,
            id: g.id,
            openId: g.isDir ? g.id : c.id,
            name: g.name,
            size: g.size,
            label: CATEGORY_LABELS[gc],
          });
        }
      } else {
        block(ctx, p, COLORS[cat], c.name, sub);
        hits.push({ ...p, id: c.id, openId: c.isDir ? c.id : null, name: c.name, size: c.size, label: CATEGORY_LABELS[cat] });
      }
    }
  }

  function hitAt(ev: MouseEvent): Hit | null {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    // Later entries are drawn on top (nested contents after their folder).
    for (let i = hits.length - 1; i >= 0; i--) {
      const h = hits[i]!;
      if (x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h) return h;
    }
    return null;
  }

  function onMove(ev: MouseEvent) {
    const hit = hitAt(ev);
    const rect = container.getBoundingClientRect();
    hover = hit ? { x: ev.clientX - rect.left, y: ev.clientY - rect.top, hit } : null;
  }

  function onClick(ev: MouseEvent) {
    const hit = hitAt(ev);
    if (hit?.openId != null) onOpen(hit.openId);
  }

  let legend = $derived.by(() => {
    if (!data) return [] as Category[];
    const seen = new Set<Category>();
    const visit = (n: TreemapNode) => {
      for (const c of n.children) {
        seen.add(category(c.name, c.isDir));
        c.children.forEach((g) => seen.add(category(g.name, g.isDir)));
      }
    };
    visit(data);
    return (Object.keys(COLORS) as Category[]).filter((c) => seen.has(c));
  });
</script>

<div class="wrap">
  <div class="treemap" bind:this={container}>
    <!-- A visual aid for mouse users; the folder tree and table below give
         the same information and actions to keyboard and screen reader users. -->
    <canvas
      bind:this={canvas}
      aria-hidden="true"
      onmousemove={onMove}
      onmouseleave={() => (hover = null)}
      onclick={onClick}
    ></canvas>
    <p class="sr-only">
      {data ? `Treemap of ${data.name}. Each block's area shows its size. The table below lists the same items.` : ""}
    </p>
    {#if hover}
      <div
        class="tip"
        style:left={`${Math.min(hover.x + 14, width - 230)}px`}
        style:top={`${Math.min(hover.y + 14, height - 60)}px`}
      >
        <strong>{hover.hit.name}</strong>
        <span>{formatSize(hover.hit.size)} · {data ? formatPercent(hover.hit.size, data.size) : ""} of this folder</span>
        <span class="muted">{hover.hit.label}{hover.hit.openId != null ? " · click to open" : ""}</span>
      </div>
    {/if}
    {#if error}<p class="muted err">{error}</p>{/if}
  </div>
  {#if legend.length}
    <ul class="legend" aria-label="Treemap colors">
      {#each legend as c (c)}
        <li><span class="swatch" style:background={c === "folder" ? COLORS.folder : COLORS[c]}></span>{CATEGORY_LABELS[c]}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: 6px;
    height: 100%;
    min-height: 0;
  }
  .treemap {
    position: relative;
    flex: 1;
    min-height: 120px;
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--surface-2);
  }
  canvas {
    display: block;
    cursor: pointer;
  }
  .tip {
    position: absolute;
    pointer-events: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-width: 220px;
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    box-shadow: var(--shadow);
    font-size: 12px;
    word-break: break-all;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 12px;
    color: var(--text-2);
  }
  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }
  .err {
    position: absolute;
    inset: 8px;
  }
</style>
