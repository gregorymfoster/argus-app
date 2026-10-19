// Paints a fake "photo" for a Photo onto a canvas and caches it as a JPEG
// data URL. Stands in for the Engine's preview path.

import type { Capture, Person, Photo, Scene } from '../domain/types';
import { GROUND_Y, personGeometry, rng } from './dummy';

type Ctx = CanvasRenderingContext2D;

function sky(ctx: Ctx, w: number, h: number, top: string, bottom: string, until: number) {
  const g = ctx.createLinearGradient(0, 0, 0, h * until);
  g.addColorStop(0, top);
  g.addColorStop(1, bottom);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, w, h * until + 1);
}

function ridge(ctx: Ctx, w: number, h: number, r: () => number, base: number, amp: number, color: string, peaks: number) {
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(0, h);
  ctx.lineTo(0, h * base);
  for (let i = 0; i <= peaks; i++) {
    const x = (w * i) / peaks;
    const y = h * (base - amp * (0.3 + r() * 0.7));
    ctx.lineTo(x - w / peaks / 2, y);
    ctx.lineTo(x, h * base - amp * h * 0.15 * r());
  }
  ctx.lineTo(w, h * base);
  ctx.lineTo(w, h);
  ctx.closePath();
  ctx.fill();
}

function background(ctx: Ctx, scene: Scene, w: number, h: number) {
  const r = rng(scene.seed);
  switch (scene.kind) {
    case 'lake': {
      sky(ctx, w, h, '#6FA3D6', '#D9E6EE', 0.45);
      ridge(ctx, w, h, r, 0.44, 0.16, '#7D8FA6', 7);
      ridge(ctx, w, h, r, 0.46, 0.08, '#5D7186', 11);
      const g = ctx.createLinearGradient(0, h * 0.46, 0, h * 0.7);
      g.addColorStop(0, '#3F6F8F');
      g.addColorStop(1, '#2C5470');
      ctx.fillStyle = g;
      ctx.fillRect(0, h * 0.46, w, h * 0.24);
      ctx.fillStyle = 'rgba(255,255,255,0.18)';
      for (let i = 0; i < 18; i++) ctx.fillRect(r() * w, h * (0.48 + r() * 0.2), w * (0.04 + r() * 0.1), Math.max(1, h * 0.004));
      ctx.fillStyle = '#CDB89A';
      ctx.fillRect(0, h * 0.7, w, h * 0.3);
      break;
    }
    case 'forest': {
      sky(ctx, w, h, '#A9C7D9', '#E3ECE6', 0.32);
      ctx.fillStyle = '#4C5E3F';
      ctx.fillRect(0, h * 0.3, w, h * 0.7);
      for (let i = 0; i < 26; i++) {
        const x = r() * w;
        const tw = w * (0.05 + r() * 0.06);
        const top = h * (0.05 + r() * 0.2);
        ctx.fillStyle = ['#23362A', '#2D4434', '#1D2E24'][i % 3];
        ctx.beginPath();
        ctx.moveTo(x, top);
        ctx.lineTo(x - tw, h * 0.72);
        ctx.lineTo(x + tw, h * 0.72);
        ctx.closePath();
        ctx.fill();
      }
      ctx.fillStyle = '#5E4B35';
      ctx.fillRect(0, h * 0.72, w, h * 0.28);
      break;
    }
    case 'cabin': {
      ctx.fillStyle = '#8C6A4F';
      ctx.fillRect(0, 0, w, h);
      ctx.fillStyle = 'rgba(0,0,0,0.12)';
      for (let i = 0; i < 14; i++) ctx.fillRect(0, (h * 0.68 * i) / 14, w, Math.max(1, h * 0.004));
      const wx = w * (0.08 + r() * 0.5);
      sky(ctx, w, h, '#BFD8EE', '#EEF4F8', 0);
      ctx.fillStyle = '#E8F1F8';
      ctx.fillRect(wx, h * 0.12, w * 0.3, h * 0.34);
      ctx.strokeStyle = '#5A4232';
      ctx.lineWidth = Math.max(2, w * 0.008);
      ctx.strokeRect(wx, h * 0.12, w * 0.3, h * 0.34);
      ctx.beginPath();
      ctx.moveTo(wx + w * 0.15, h * 0.12);
      ctx.lineTo(wx + w * 0.15, h * 0.46);
      ctx.stroke();
      const lamp = ctx.createRadialGradient(w * 0.85, h * 0.3, 0, w * 0.85, h * 0.3, w * 0.35);
      lamp.addColorStop(0, 'rgba(255,200,120,0.55)');
      lamp.addColorStop(1, 'rgba(255,200,120,0)');
      ctx.fillStyle = lamp;
      ctx.fillRect(0, 0, w, h);
      ctx.fillStyle = '#5B3F2C';
      ctx.fillRect(0, h * 0.68, w, h * 0.32);
      break;
    }
    case 'dusk': {
      sky(ctx, w, h, '#3B4A7A', '#F0A46B', 0.52);
      const sun = ctx.createRadialGradient(w * 0.7, h * 0.48, 0, w * 0.7, h * 0.48, w * 0.25);
      sun.addColorStop(0, 'rgba(255,230,170,0.95)');
      sun.addColorStop(0.15, 'rgba(255,200,120,0.6)');
      sun.addColorStop(1, 'rgba(255,170,100,0)');
      ctx.fillStyle = sun;
      ctx.fillRect(0, 0, w, h * 0.55);
      ridge(ctx, w, h, r, 0.52, 0.08, '#3A2E3B', 9);
      ctx.fillStyle = '#2B2430';
      ctx.fillRect(0, h * 0.6, w, h * 0.4);
      break;
    }
    case 'street': {
      sky(ctx, w, h, '#9DB4C8', '#DDE3E6', 0.4);
      let x = 0;
      while (x < w) {
        const bw = w * (0.08 + r() * 0.12);
        const bh = h * (0.25 + r() * 0.3);
        ctx.fillStyle = ['#8F8478', '#A5998A', '#7B746E', '#B4A48E'][Math.floor(r() * 4)];
        ctx.fillRect(x, h * 0.62 - bh, bw, bh);
        ctx.fillStyle = 'rgba(40,50,60,0.45)';
        for (let wy = h * 0.62 - bh + h * 0.03; wy < h * 0.58; wy += h * 0.06)
          for (let wx = x + bw * 0.12; wx < x + bw * 0.85; wx += bw * 0.25) ctx.fillRect(wx, wy, bw * 0.12, h * 0.03);
        x += bw;
      }
      ctx.fillStyle = '#6E6A66';
      ctx.fillRect(0, h * 0.62, w, h * 0.38);
      ctx.fillStyle = '#85807A';
      ctx.fillRect(0, h * 0.62, w, h * 0.05);
      break;
    }
  }
}

function shade(hex: string, k: number) {
  const n = parseInt(hex.slice(1), 16);
  const f = (c: number) => Math.max(0, Math.min(255, Math.round(c * k)));
  return `rgb(${f(n >> 16)},${f((n >> 8) & 255)},${f(n & 255)})`;
}

function person(ctx: Ctx, p: Person, w: number, h: number, ground: number, faceShadow: number) {
  const { headR, bodyH, headY } = personGeometry(p, ground);
  const cx = p.x * w;
  const R = headR * w;
  const hy = headY * h;
  const shoulderW = R * 2.6;
  const top = hy + R * 1.25;
  const bottom = ground * h + h * 0.05;
  // Body
  ctx.fillStyle = shade(p.shirt, 1 - faceShadow * 0.35);
  ctx.beginPath();
  ctx.moveTo(cx - shoulderW * 0.55, top + R * 0.3);
  ctx.quadraticCurveTo(cx, top - R * 0.4, cx + shoulderW * 0.55, top + R * 0.3);
  ctx.lineTo(cx + shoulderW * 0.75, bottom);
  ctx.lineTo(cx - shoulderW * 0.75, bottom);
  ctx.closePath();
  ctx.fill();
  void bodyH;
  // Neck
  ctx.fillStyle = shade(p.skin, 0.82 - faceShadow * 0.45);
  ctx.fillRect(cx - R * 0.35, hy + R * 0.7, R * 0.7, R * 0.7);
  // Head
  ctx.fillStyle = shade(p.skin, 1 - faceShadow * 0.5);
  ctx.beginPath();
  ctx.ellipse(cx, hy, R * 0.86, R, 0, 0, Math.PI * 2);
  ctx.fill();
  // Hair
  ctx.fillStyle = p.hair;
  ctx.beginPath();
  ctx.ellipse(cx, hy - R * 0.35, R * 0.92, R * 0.7, 0, Math.PI, Math.PI * 2);
  ctx.fill();
  // Eyes
  ctx.fillStyle = '#1B1B1E';
  ctx.strokeStyle = '#1B1B1E';
  ctx.lineWidth = Math.max(1, R * 0.08);
  for (const dx of [-0.32, 0.32]) {
    if (p.eyesClosed) {
      ctx.beginPath();
      ctx.moveTo(cx + dx * R - R * 0.13, hy + R * 0.02);
      ctx.lineTo(cx + dx * R + R * 0.13, hy + R * 0.02);
      ctx.stroke();
    } else {
      ctx.beginPath();
      ctx.arc(cx + dx * R, hy, R * 0.09, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  // Mouth
  ctx.beginPath();
  if (p.smile) ctx.arc(cx, hy + R * 0.3, R * 0.28, 0.15 * Math.PI, 0.85 * Math.PI);
  else {
    ctx.moveTo(cx - R * 0.18, hy + R * 0.48);
    ctx.lineTo(cx + R * 0.18, hy + R * 0.48);
  }
  ctx.stroke();
}

function paintScene(ctx: Ctx, photo: Photo, w: number, h: number) {
  const { scene, capture } = photo;
  ctx.save();
  ctx.translate(capture.shift * w, 0);
  background(ctx, scene, w * 1.02, h);
  const ground = GROUND_Y[scene.kind];
  // Back to front: smaller people are further away.
  [...scene.people].sort((a, b) => a.scale - b.scale).forEach((p) => person(ctx, p, w, h, ground, capture.faceShadow));
  ctx.restore();
}

function applyCapture(ctx: Ctx, src: HTMLCanvasElement, cap: Capture, w: number, h: number) {
  const scale = w / 1000;
  const bright = Math.pow(2, cap.ev * 0.55);
  const warm = cap.wb;
  const parts = [`brightness(${bright})`];
  if (cap.blur > 0) parts.push(`blur(${cap.blur * scale}px)`);
  ctx.filter = parts.join(' ');
  if (cap.motion > 0) {
    const steps = 6;
    ctx.globalAlpha = 1 / steps;
    for (let i = 0; i < steps; i++) ctx.drawImage(src, ((i - steps / 2) * cap.motion * scale) / steps, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'destination-over';
    ctx.drawImage(src, 0, 0);
    ctx.globalCompositeOperation = 'source-over';
  } else {
    ctx.drawImage(src, 0, 0);
  }
  ctx.filter = 'none';
  if (Math.abs(warm) > 0.05) {
    ctx.globalCompositeOperation = 'soft-light';
    ctx.fillStyle = warm > 0 ? `rgba(255,150,50,${warm * 0.6})` : `rgba(60,130,255,${-warm * 0.6})`;
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = 'source-over';
  }
  // A touch of vignette so frames read as photos.
  const v = ctx.createRadialGradient(w / 2, h / 2, h * 0.4, w / 2, h / 2, w * 0.75);
  v.addColorStop(0, 'rgba(0,0,0,0)');
  v.addColorStop(1, 'rgba(0,0,0,0.28)');
  ctx.fillStyle = v;
  ctx.fillRect(0, 0, w, h);
}

const cache = new Map<string, string>();

export function previewUrl(photo: Photo, size: 'thumb' | 'full'): string {
  if (photo.preview) return photo.preview;
  const key = `${photo.id}:${size}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const w = size === 'thumb' ? 480 : 1500;
  const h = Math.round((w * 2) / 3);
  const a = document.createElement('canvas');
  a.width = w;
  a.height = h;
  paintScene(a.getContext('2d')!, photo, w, h);
  const b = document.createElement('canvas');
  b.width = w;
  b.height = h;
  applyCapture(b.getContext('2d')!, a, photo.capture, w, h);
  const url = b.toDataURL('image/jpeg', size === 'thumb' ? 0.82 : 0.9);
  cache.set(key, url);
  return url;
}
