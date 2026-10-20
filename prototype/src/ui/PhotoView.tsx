import { memo, useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { previewUrl } from '../data/painter';
import { layerStyle, plan } from '../domain/render';
import { TILE_EDGE, useEdited } from '../data/editedThumbs';
import { LIVE } from '../data/live';
import type { Look, Photo, Recipe } from '../domain/types';

const GRAIN =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E\")";

const IDENTITY = layerStyle({}).filter;

/** `tile`: the grid's small thumbnail; `thumb`: the preview; `full`: the preview, painted at once. */
type Size = 'tile' | 'thumb' | 'full';

export const MASK_COLORS = ['#FFB27A', '#8EC5FF', '#B9A4FF', '#7FD1A8', '#F48FB1'];

/** The grid's small copy of a preview, made by the engine on first ask (`previews/thumbs/`). */
const thumbOf = (url: string) => url.replace('/previews/', '/previews/thumbs/');

/** A preview's URL: real ones at once, painted stand-ins lazily so a grid of 112 doesn't block the first frame. */
export function usePreview(photo: Photo, size: Size) {
  const real = photo.preview ? (size === 'tile' ? thumbOf(photo.preview) : photo.preview) : null;
  const [url, setUrl] = useState<string | null>(real);
  useEffect(() => {
    if (real) return setUrl(real);
    let live = true;
    const run = () => live && setUrl(previewUrl(photo, size === 'full' ? 'full' : 'thumb'));
    const w = window as Window & { requestIdleCallback?: (cb: () => void) => number };
    if (size === 'full') run();
    else if (w.requestIdleCallback) w.requestIdleCallback(run);
    else setTimeout(run, 0);
    return () => {
      live = false;
    };
  }, [photo, size, real]);
  return url;
}

function Layer({ url, adjust, mask, alt }: { url: string; adjust: ReturnType<typeof layerStyle>; mask?: string; alt: string }) {
  const style: CSSProperties = mask ? { WebkitMaskImage: mask, maskImage: mask } : {};
  // Never a broken-image icon: a preview that isn't there yet (still rendering, or just
  // swapped for Argus's own) shimmers and is asked for again.
  const [tries, setTries] = useState(0);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setTries(0);
    setFailed(false);
  }, [url]);
  useEffect(() => {
    if (!failed || tries >= 4) return;
    const t = setTimeout(() => (setTries((n) => n + 1), setFailed(false)), 1000 * 2 ** tries);
    return () => clearTimeout(t);
  }, [failed, tries]);
  const src = tries && !url.startsWith('data:') ? `${url}${url.includes('?') ? '&' : '?'}retry=${tries}` : url;
  return (
    <div className="layer" style={style}>
      {failed ? (
        <div className="shimmer" data-testid="preview-pending" title={tries >= 4 ? 'Preview not available' : 'Preview on its way'} />
      ) : (
        <img src={src} alt={alt} style={adjust.filter === IDENTITY ? undefined : { filter: adjust.filter }} draggable={false} onError={() => setFailed(true)} />
      )}
      {adjust.overlays.map((o, i) => (
        <div key={i} className="overlay" style={{ background: o.background, mixBlendMode: o.blend as CSSProperties['mixBlendMode'], opacity: o.opacity }} />
      ))}
    </div>
  );
}

interface Props {
  photo: Photo;
  size?: Size;
  recipe?: Recipe;
  look?: Look | null;
  original?: boolean;
  showMasks?: boolean;
  /** Only render the mask tint for this local id. */
  maskFocus?: string | null;
  /** An image the engine already rendered with `recipe` (Masks included): shown instead of the CSS stand-in. */
  rendered?: string | null;
  /** The engine's Masks as alpha images by local id, for the Mask tint over `rendered`. */
  engineMasks?: Record<string, string>;
  /** Live: show the Photo as the engine renders it with its saved Recipe (grid, filmstrip, Loupe, Compare). */
  edits?: boolean;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
  onPointerDown?: (e: React.PointerEvent<HTMLDivElement>) => void;
  onPointerMove?: (e: React.PointerEvent<HTMLDivElement>) => void;
  onPointerUp?: (e: React.PointerEvent<HTMLDivElement>) => void;
}

export const PhotoView = memo(function PhotoView({ photo, size = 'thumb', recipe, look, original, showMasks, maskFocus, rendered, engineMasks, edits, className, style, children, ...pointer }: Props) {
  const preview = usePreview(photo, size);
  const edited = useEdited(photo, Boolean(edits) && LIVE && !rendered && !original, size === 'full' ? 1536 : TILE_EDGE);
  const engine = Boolean(rendered || edited) && !original;
  const url = engine ? (rendered ?? edited)! : preview;
  const p = recipe && !original ? plan(photo, recipe, look ?? null) : null;
  return (
    <div className={`photo ${className ?? ''}`} style={style} {...pointer}>
      {url ? (
        <>
          <Layer url={url} adjust={layerStyle(engine ? {} : (p?.base ?? {}))} alt={photo.file} />
          {/* The engine renders local adjustments itself; CSS layers stand in for dummy Photos. */}
          {!engine && p?.locals.map((l) => <Layer key={l.local.id} url={url} adjust={layerStyle(l.adjust)} mask={l.mask} alt="" />)}
          {p && p.grain > 0 && <div className="overlay" style={{ backgroundImage: GRAIN, opacity: p.grain / 180, mixBlendMode: 'overlay' }} />}
          {showMasks &&
            p?.locals
              .filter((l) => !maskFocus || l.local.id === maskFocus)
              .map((l, i) => {
                const m = engine ? (engineMasks?.[l.local.id] ? `url(${engineMasks[l.local.id]})` : null) : l.mask;
                if (!m) return null;
                return (
                  <div
                    key={`m-${l.local.id}`}
                    className="overlay"
                    data-testid="mask-tint"
                    style={{ background: MASK_COLORS[(recipe!.local.findIndex((x) => x.id === l.local.id) + MASK_COLORS.length) % MASK_COLORS.length] ?? MASK_COLORS[i], opacity: 0.38, WebkitMaskImage: m, maskImage: m, WebkitMaskSize: '100% 100%', maskSize: '100% 100%' }}
                  />
                );
              })}
        </>
      ) : (
        <div className="shimmer" />
      )}
      {children}
    </div>
  );
});
