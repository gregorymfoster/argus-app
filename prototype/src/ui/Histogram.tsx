// Lightroom's histogram, small: luminance plus the red, green and blue channels, read
// from the preview the person is looking at. A preview the page can't read hides it.

import { useEffect, useState } from 'react';

const BINS = 64;
const W = 276;
const H = 56;

type Hist = { l: number[]; r: number[]; g: number[]; b: number[] };

function read(img: HTMLImageElement): Hist | null {
  const c = document.createElement('canvas');
  const scale = Math.min(1, 160 / Math.max(img.naturalWidth, img.naturalHeight));
  c.width = Math.max(1, Math.round(img.naturalWidth * scale));
  c.height = Math.max(1, Math.round(img.naturalHeight * scale));
  const ctx = c.getContext('2d', { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(img, 0, 0, c.width, c.height);
  let data: Uint8ClampedArray;
  try {
    data = ctx.getImageData(0, 0, c.width, c.height).data;
  } catch {
    return null; // A cross-origin preview: nothing to read.
  }
  const h: Hist = { l: Array(BINS).fill(0), r: Array(BINS).fill(0), g: Array(BINS).fill(0), b: Array(BINS).fill(0) };
  const bin = (v: number) => Math.min(BINS - 1, (v * BINS) >> 8);
  for (let i = 0; i < data.length; i += 4) {
    const [r, g, b] = [data[i], data[i + 1], data[i + 2]];
    h.r[bin(r)]++;
    h.g[bin(g)]++;
    h.b[bin(b)]++;
    h.l[bin(0.2126 * r + 0.7152 * g + 0.0722 * b)]++;
  }
  return h;
}

function area(bins: number[], max: number): string {
  const step = W / (BINS - 1);
  const pts = bins.map((v, i) => `${(i * step).toFixed(1)},${(H - (Math.sqrt(v / max) * (H - 2))).toFixed(1)}`);
  return `0,${H} ${pts.join(' ')} ${W},${H}`;
}

export function Histogram({ src, exif }: { src: string | null | undefined; exif?: string }) {
  const [h, setH] = useState<Hist | null>(null);
  useEffect(() => {
    setH(null);
    if (!src) return;
    let on = true;
    const img = new Image();
    if (!src.startsWith('data:')) img.crossOrigin = 'anonymous';
    img.onload = () => on && setH(read(img));
    img.src = src;
    return () => {
      on = false;
    };
  }, [src]);
  if (!h) return null;
  // Square roots tame a spike (a clipped sky) so the rest still reads.
  const max = Math.max(1, ...h.r, ...h.g, ...h.b, ...h.l);
  const [shutter, aperture, iso, focal] = (exif ?? '').split(' · ');
  return (
    <div className="histogram" data-testid="histogram">
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-label="Histogram">
        <polygon points={area(h.r, max)} fill="rgb(240 113 107 / 0.22)" />
        <polygon points={area(h.g, max)} fill="rgb(122 196 140 / 0.2)" />
        <polygon points={area(h.b, max)} fill="rgb(122 166 231 / 0.24)" />
        <polygon points={area(h.l, max)} fill="rgb(240 239 236 / 0.16)" />
      </svg>
      {exif && (
        <div className="histogram-exif mono">
          <span>{iso}</span>
          <span>{focal}</span>
          <span>{aperture}</span>
          <span>{shutter}</span>
        </div>
      )}
    </div>
  );
}
