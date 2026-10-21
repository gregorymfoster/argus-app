// Lightroom's Navigator (a mini-map of the photo with the visible area boxed, Fit / Fill /
// 50 / 100 / 200 across its head) and the zoom readout on the stage.

import { useRef, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/menu";
import { Kbd } from "./kit";
import { PRESETS, type Preset, type ZoomPan, useZoomView } from "./zoomPan";

const label = (p: Preset) =>
  p === "fit" ? "Fit" : p === "fill" ? "Fill" : `${p}%`;

/** Which preset the view is at, if any. */
function current(z: ZoomPan, s: number): Preset | null {
  return PRESETS.find((p) => Math.abs(z.presetScale(p) - s) / s < 0.01) ?? null;
}

// Closed until the person opens it, so Edit's sliders get the room (Greg, 2026-10-21).
const OPEN = "argus-navigator";
function remembered() {
  try {
    return localStorage.getItem(OPEN) === "open";
  } catch {
    return false;
  }
}

export function Navigator({
  z,
  children,
}: {
  z: ZoomPan;
  children: React.ReactNode;
}) {
  const v = useZoomView(z);
  const [open, setOpen] = useState(remembered);
  const toggle = () => {
    setOpen(!open);
    try {
      localStorage.setItem(OPEN, open ? "closed" : "open");
    } catch {
      /* not remembered */
    }
  };
  const at = current(z, v.s);
  const box = useRef<HTMLDivElement>(null);
  const seek = (e: React.PointerEvent) => {
    const b = box.current!.getBoundingClientRect();
    z.centerOn(
      Math.min(1, Math.max(0, (e.clientX - b.left) / b.width)),
      Math.min(1, Math.max(0, (e.clientY - b.top) / b.height)),
    );
  };
  return (
    <div className="navigator" data-testid="navigator">
      <div className="nav-head">
        <button className="nav-title" onClick={toggle} aria-expanded={open}>
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}{" "}
          Navigator
        </button>
        <span className="grow" />
        {PRESETS.map((p) => (
          <button
            key={String(p)}
            className={at === p ? "on" : ""}
            onClick={() => z.preset(p)}
            data-testid={`zoom-${p}`}
          >
            {label(p)}
          </button>
        ))}
      </div>
      {open && (
        <div
          ref={box}
          className="nav-map"
          onPointerDown={(e) => {
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
            seek(e);
          }}
          onPointerMove={(e) => e.buttons === 1 && seek(e)}
        >
          {children}
          {v.s > 1.001 && (
            <span
              className="nav-view"
              data-testid="nav-view"
              style={{
                left: `${v.rel.x * 100}%`,
                top: `${v.rel.y * 100}%`,
                width: `${v.rel.w * 100}%`,
                height: `${v.rel.h * 100}%`,
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}

/** The zoom level on the stage, with the presets a click away. */
export function ZoomReadout({ z, loading }: { z: ZoomPan; loading?: boolean }) {
  const v = useZoomView(z);
  const at = current(z, v.s);
  const text =
    v.s <= 1.001 ? "Fit" : at === "fill" ? "Fill" : `${z.percent()}%`;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          className={`zoom-readout ${v.s > 1.001 ? "in" : ""}`}
          data-testid="zoom-readout"
          title="Zoom (Z, ⌘+ / ⌘−, pinch)"
        >
          <span className="mono">{text}</span>
          {loading && (
            <span
              className="zoom-sharpening"
              data-testid="zoom-loading"
              title="Loading full size"
            />
          )}
          <ChevronDown size={10} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="top" className="min-w-[160px]">
        {PRESETS.map((p) => (
          <DropdownMenuItem key={String(p)} onSelect={() => z.preset(p)}>
            <span className="grow">{label(p)}</span>
            {p === "fit" && <Kbd>⌘0</Kbd>}
            {p === 100 && <Kbd>Z</Kbd>}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
