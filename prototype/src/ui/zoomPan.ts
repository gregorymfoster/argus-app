// Lightroom's zoom and pan for the Loupe and Edit stages: trackpad pinch (ctrl+wheel in
// Chromium, gesture events in WebKit) zooms around the pointer, two-finger scroll pans,
// Space-drag is the hand tool, drag pans when zoomed, click or Z toggles Fit and 100% at
// the clicked point, ⌘+ / ⌘− step and ⌘0 fits. 100% is one image pixel per screen pixel.
//
// The view is applied straight to the photo element's transform, so a gesture never
// re-renders the screen; components that show it (Navigator, readout) subscribe.

import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import { useHotkeys } from "react-hotkeys-hook";
import { routeZoom } from "./zoom";

export interface View {
  /** Scale against Fit (1 = the whole photo fits the stage). */
  s: number;
  /** Where the photo's top-left corner sits in the stage, in CSS px. */
  x: number;
  y: number;
}

export type Preset = "fit" | "fill" | 50 | 100 | 200;
export const PRESETS: Preset[] = ["fit", "fill", 50, 100, 200];
/** ⌘+ / ⌘− stops, as percentages of 100%; Fit sits wherever it falls among them. */
const STOPS = [25, 33, 50, 67, 100, 200, 300, 400, 800];

export class ZoomPan {
  stage: HTMLElement | null = null;
  view: View = { s: 1, x: 0, y: 0 };
  /** The photo's long edge in pixels, for 100%. */
  natural = 6000;
  /** Space is held: drag pans whatever tool is active. */
  hand = false;
  dragging = false;
  private subs = new Set<() => void>();
  private snap = {
    s: 1,
    rel: { x: 0, y: 0, w: 1, h: 1 },
    hand: false,
    dragging: false,
  };
  private frame = 0;

  photo(): HTMLElement | null {
    return (
      this.stage?.querySelector<HTMLElement>(".stage-fit > .photo") ?? null
    );
  }

  /** The photo's fitted box in the stage (layout ignores the transform). */
  fit() {
    const p = this.photo();
    if (!p) return { px: 0, py: 0, fw: 1, fh: 1 };
    return {
      px: p.offsetLeft,
      py: p.offsetTop,
      fw: p.offsetWidth || 1,
      fh: p.offsetHeight || 1,
    };
  }

  size() {
    return {
      W: this.stage?.clientWidth ?? 1,
      H: this.stage?.clientHeight ?? 1,
    };
  }

  /** The scale (against Fit) that shows the photo at `pct` percent. */
  scaleFor(pct: number) {
    const { fw } = this.fit();
    return (this.natural * (pct / 100)) / (fw * (window.devicePixelRatio || 1));
  }

  percent() {
    return Math.round((100 / this.scaleFor(100)) * this.view.s);
  }

  presetScale(p: Preset) {
    if (p === "fit") return 1;
    if (p === "fill") {
      const { fw, fh } = this.fit();
      const { W, H } = this.size();
      return Math.max(W / fw, H / fh);
    }
    return this.scaleFor(p);
  }

  zoomed() {
    return this.view.s > 1.001;
  }

  private bounds() {
    return {
      lo: Math.min(1, this.scaleFor(25)),
      hi: Math.max(2, this.scaleFor(800)),
    };
  }

  /** Keep the photo covering the stage when it's bigger, centred when it's smaller. */
  private clamp(v: View): View {
    const { px, py, fw, fh } = this.fit();
    const { W, H } = this.size();
    const axis = (pos: number, len: number, room: number, at: number) => {
      if (len <= room) return v.s === 1 ? at : (room - len) / 2;
      return Math.min(0, Math.max(room - len, pos));
    };
    return {
      s: v.s,
      x: axis(v.x, fw * v.s, W, px),
      y: axis(v.y, fh * v.s, H, py),
    };
  }

  set(v: View, animate = false) {
    this.view = this.clamp(v);
    const p = this.photo();
    if (p) {
      const { px, py } = this.fit();
      p.style.transition = animate
        ? "transform 200ms cubic-bezier(0.2, 0.7, 0.2, 1)"
        : "none";
      p.style.transformOrigin = "0 0";
      p.style.transform =
        this.view.s === 1
          ? ""
          : `translate(${this.view.x - px}px, ${this.view.y - py}px) scale(${this.view.s})`;
      p.style.setProperty("--zoom", String(this.view.s));
    }
    if (this.stage) this.stage.dataset.zoom = this.zoomed() ? "in" : "fit";
    this.notify();
  }

  /** Zoom to scale `s`, keeping the stage point (cx, cy) still (the centre by default). */
  zoomAt(s: number, cx?: number, cy?: number, animate = false) {
    const { lo, hi } = this.bounds();
    const next = Math.min(hi, Math.max(lo, s));
    const { W, H } = this.size();
    const x0 = cx ?? W / 2;
    const y0 = cy ?? H / 2;
    const k = next / this.view.s;
    if (Math.abs(next - 1) < 0.002)
      return this.set({ s: 1, x: 0, y: 0 }, animate);
    this.set(
      {
        s: next,
        x: x0 - (x0 - this.view.x) * k,
        y: y0 - (y0 - this.view.y) * k,
      },
      animate,
    );
  }

  preset(p: Preset) {
    this.zoomAt(this.presetScale(p), undefined, undefined, true);
  }

  /** Z, click: Fit when zoomed, else 100% around the point. */
  toggle(cx?: number, cy?: number) {
    if (this.zoomed()) this.zoomAt(1, cx, cy, true);
    else this.zoomAt(Math.max(this.scaleFor(100), 1.5), cx, cy, true);
  }

  step(dir: "in" | "out" | "reset") {
    if (dir === "reset") return this.zoomAt(1, undefined, undefined, true);
    const s = this.view.s;
    const stops = [1, ...STOPS.map((pct) => this.scaleFor(pct))].sort(
      (a, b) => a - b,
    );
    const next =
      dir === "in"
        ? stops.find((x) => x > s * 1.01)
        : [...stops].reverse().find((x) => x < s / 1.01);
    if (next != null) this.zoomAt(next, undefined, undefined, true);
  }

  panBy(dx: number, dy: number) {
    if (!this.zoomed()) return;
    this.set({ ...this.view, x: this.view.x + dx, y: this.view.y + dy });
  }

  /** Centre the view on a point of the photo, in 0-1 photo coordinates (the Navigator). */
  centerOn(u: number, v: number) {
    const { fw, fh } = this.fit();
    const { W, H } = this.size();
    const s = this.zoomed() ? this.view.s : this.presetScale(100);
    this.set(
      { s, x: W / 2 - u * fw * s, y: H / 2 - v * fh * s },
      !this.zoomed(),
    );
  }

  /** Re-apply after a layout change (window resize, next photo): same scale, same centre. */
  refresh() {
    const { W, H } = this.size();
    const { fw, fh } = this.fit();
    const r = this.snap.rel;
    if (!this.zoomed()) return this.set({ s: 1, x: 0, y: 0 });
    const s = this.view.s;
    this.set({
      s,
      x: W / 2 - (r.x + r.w / 2) * fw * s,
      y: H / 2 - (r.y + r.h / 2) * fh * s,
    });
  }

  /** The visible part of the photo, in 0-1 photo coordinates. */
  private visible() {
    const { fw, fh } = this.fit();
    const { W, H } = this.size();
    const { s, x, y } = this.view;
    const x0 = Math.max(0, -x / (fw * s));
    const y0 = Math.max(0, -y / (fh * s));
    const x1 = Math.min(1, (W - x) / (fw * s));
    const y1 = Math.min(1, (H - y) / (fh * s));
    return { x: x0, y: y0, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0) };
  }

  private notify() {
    this.snap = {
      s: this.view.s,
      rel: this.zoomed() ? this.visible() : { x: 0, y: 0, w: 1, h: 1 },
      hand: this.hand,
      dragging: this.dragging,
    };
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.subs.forEach((f) => f());
    });
  }

  setHand(on: boolean) {
    if (this.hand === on) return;
    this.hand = on;
    if (this.stage) this.stage.dataset.hand = on ? "on" : "";
    this.notify();
  }

  subscribe = (f: () => void) => {
    this.subs.add(f);
    return () => void this.subs.delete(f);
  };

  snapshot = () => this.snap;
}

/** The current view, re-rendering only the component that asks. */
export function useZoomView(z: ZoomPan) {
  return useSyncExternalStore(z.subscribe, z.snapshot);
}

interface Options {
  /** A click toggles Fit / 100% (off while painting a Mask or cropping). */
  click?: boolean;
  /** Changing it keeps the zoom and moves to the same spot of the next photo, as Lightroom. */
  photoKey: string;
  /** The photo's long edge in pixels. */
  natural?: number;
}

/**
 * Wire a stage element for zoom and pan. Spread `stageProps` on the `.stage` and give it
 * `ref`; hotkeys (Z, Space, ⌘+ / ⌘− / ⌘0) are global while the stage is mounted.
 */
export function useZoomPan({ click = true, photoKey, natural }: Options) {
  const z = useMemo(() => new ZoomPan(), []);
  const opts = useRef({ click });
  opts.current = { click };
  if (natural) z.natural = natural;

  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    z.stage = el;
    const point = (e: { clientX: number; clientY: number }) => {
      const r = el.getBoundingClientRect();
      // The app's own CSS zoom (browser fallback) scales client coordinates.
      const k = el.offsetWidth ? r.width / el.offsetWidth : 1;
      return [(e.clientX - r.left) / k, (e.clientY - r.top) / k] as const;
    };
    const wheel = (e: WheelEvent) => {
      const unit =
        e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? el.clientHeight : 1;
      if (e.ctrlKey) {
        e.preventDefault();
        const [cx, cy] = point(e);
        z.zoomAt(z.view.s * Math.exp(-e.deltaY * unit * 0.01), cx, cy);
      } else if (z.zoomed()) {
        e.preventDefault();
        z.panBy(-e.deltaX * unit, -e.deltaY * unit);
      }
    };
    type Gesture = Event & { scale: number; clientX: number; clientY: number };
    let start = 1;
    const gestureStart = (e: Event) => {
      e.preventDefault();
      start = z.view.s;
    };
    const gestureChange = (e: Event) => {
      e.preventDefault();
      const g = e as Gesture;
      const [cx, cy] = point(g);
      z.zoomAt(start * g.scale, cx, cy);
    };
    // Space is the hand tool, as everywhere in Adobe's apps; it never reaches a button.
    const typing = (t: EventTarget | null) =>
      t instanceof HTMLElement &&
      (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
    const keydown = (e: KeyboardEvent) => {
      if (e.code !== "Space" || typing(e.target)) return;
      e.preventDefault();
      z.setHand(true);
    };
    const keyup = (e: KeyboardEvent) => e.code === "Space" && z.setHand(false);
    const blur = () => z.setHand(false);
    const ro = new ResizeObserver(() => z.refresh());
    ro.observe(el);
    el.addEventListener("wheel", wheel, { passive: false });
    el.addEventListener("gesturestart", gestureStart);
    el.addEventListener("gesturechange", gestureChange);
    window.addEventListener("keydown", keydown);
    window.addEventListener("keyup", keyup);
    window.addEventListener("blur", blur);
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
      el.removeEventListener("gesturestart", gestureStart);
      el.removeEventListener("gesturechange", gestureChange);
      window.removeEventListener("keydown", keydown);
      window.removeEventListener("keyup", keyup);
      window.removeEventListener("blur", blur);
      z.stage = null;
    };
  }, [z]);

  // Same zoom on the next photo (its element may be new): re-apply once it has laid out.
  useEffect(() => {
    const id = requestAnimationFrame(() => z.refresh());
    return () => cancelAnimationFrame(id);
  }, [z, photoKey]);

  useHotkeys("z", () => z.toggle(), [z]);
  useHotkeys(
    ["mod+equal", "mod+shift+equal"],
    () => z.step("in"),
    { preventDefault: true },
    [z],
  );
  useHotkeys("mod+minus", () => z.step("out"), { preventDefault: true }, [z]);
  useHotkeys("mod+0", () => z.step("reset"), { preventDefault: true }, [z]);
  useEffect(() => routeZoom((step) => z.step(step)), [z]);

  const lastToggle = useRef(0);
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 && e.button !== 1) return;
    // The readout, its menu (portalled, but its events still bubble here) and buttons.
    if (
      (e.target as HTMLElement).closest("button, [role=menu], [data-no-zoom]")
    )
      return;
    const el = ref.current!;
    const panning = z.hand || e.button === 1 || z.zoomed();
    const r = el.getBoundingClientRect();
    const k = el.offsetWidth ? r.width / el.offsetWidth : 1;
    const sx = e.clientX;
    const sy = e.clientY;
    let lx = sx;
    let ly = sy;
    let lt = performance.now();
    let vx = 0;
    let vy = 0;
    let moved = false;
    if (
      panning &&
      (z.hand ||
        e.button === 1 ||
        opts.current.click ||
        !(e.target as HTMLElement).closest(".photo"))
    ) {
      el.setPointerCapture(e.pointerId);
      e.preventDefault();
    } else if (!opts.current.click) return;
    const move = (ev: PointerEvent) => {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) > 4) moved = true;
      if (!moved || !panning || !el.hasPointerCapture(e.pointerId)) return;
      if (!z.dragging) {
        z.dragging = true;
        el.dataset.drag = "on";
      }
      const now = performance.now();
      const dt = Math.max(1, now - lt);
      vx = ((ev.clientX - lx) / k / dt) * 0.6 + vx * 0.4;
      vy = ((ev.clientY - ly) / k / dt) * 0.6 + vy * 0.4;
      z.panBy((ev.clientX - lx) / k, (ev.clientY - ly) / k);
      lx = ev.clientX;
      ly = ev.clientY;
      lt = now;
    };
    const up = (ev: PointerEvent) => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.dataset.drag = "";
      z.dragging = false;
      if (moved && panning) {
        // A flick keeps gliding, like a trackpad's own momentum.
        if (performance.now() - lt > 60) return;
        let t = performance.now();
        const glide = (now: number) => {
          const dt = now - t;
          t = now;
          z.panBy(vx * dt, vy * dt);
          const decay = Math.pow(0.994, dt);
          vx *= decay;
          vy *= decay;
          if (Math.hypot(vx, vy) > 0.02 && !z.dragging)
            requestAnimationFrame(glide);
        };
        requestAnimationFrame(glide);
        return;
      }
      if (moved || z.hand || !opts.current.click || e.button !== 0) return;
      // A double-click is one toggle, not two.
      if (performance.now() - lastToggle.current < 400) return;
      lastToggle.current = performance.now();
      const b = el.getBoundingClientRect();
      z.toggle((ev.clientX - b.left) / k, (ev.clientY - b.top) / k);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  return {
    z,
    ref,
    stageProps: { ref, onPointerDown, "data-zoom-click": click ? "on" : "off" },
  };
}

/** Whether the view is zoomed past Fit; re-renders only when that flips. */
export function useZoomed(z: ZoomPan) {
  return useSyncExternalStore(z.subscribe, () => z.view.s > 1.001);
}

/** The long edge (device px) the zoomed photo now covers, in 512 px steps; 0 at Fit. */
export function useNeed(z: ZoomPan) {
  return useSyncExternalStore(z.subscribe, () => {
    if (z.view.s <= 1.001) return 0;
    const { fw, fh } = z.fit();
    return (
      Math.ceil(
        (Math.max(fw, fh) * z.view.s * (window.devicePixelRatio || 1)) / 512,
      ) * 512
    );
  });
}

/** A Photo's long edge in pixels; dummy Photos are A7R II sized. */
export function longEdge(p: { pixels?: [number, number] }) {
  return p.pixels ? Math.max(...p.pixels) : 7952;
}
