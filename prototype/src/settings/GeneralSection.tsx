// Settings › General: how dense the app looks. Interface size is the same zoom as ⌘+ / ⌘− / ⌘0,
// and Thumbnail size is the grid's own slider (and pinch, = and −), so all three stay in step.

import { useEffect, useState } from 'react';
import { Slider } from '@/components/ui/slider';
import { THUMB_DEFAULT, THUMB_MAX, THUMB_MIN } from '../screens/Cull';
import { usePref } from '../store/variants';
import { Seg } from '../ui/kit';
import { ZOOM_STEPS, onZoom, setZoom, zoomLevel } from '../ui/zoom';
import { openSettings } from './Settings';
import { Row } from './Row';
import { UsageRows } from './UsageRows';

const pct = (z: number) => `${Math.round(z * 100)}%`;

export function GeneralSection() {
  const [z, setZ] = useState(zoomLevel());
  useEffect(() => onZoom(() => setZ(zoomLevel())), []);
  const [thumb, setThumb] = usePref<string>('thumb', String(THUMB_DEFAULT));
  const size = Math.max(THUMB_MIN, Math.min(THUMB_MAX, Number(thumb) || THUMB_DEFAULT));
  return (
    <div className="set-page" data-testid="settings-general-page">
      <h3>Interface</h3>
      <Row title="Interface size" note="Text and controls everywhere. ⌘+ and ⌘− step it, ⌘0 goes back to 100%.">
        <Seg label="Interface size" value={String(z)} onChange={(v) => void setZoom(Number(v))} items={ZOOM_STEPS.map((s) => ({ value: String(s), label: pct(s) }))} />
      </Row>
      <Row title="Thumbnail size" note="The grid's tiles. Pinch, or = and −, in the grid do the same.">
        <div className="set-slider" data-testid="settings-thumb">
          <Slider aria-label="Thumbnail size" min={THUMB_MIN} max={THUMB_MAX} step={1} value={[size]} onValueChange={([v]) => setThumb(String(v))} />
        </div>
        <span className="mono faint set-num">{size} px</span>
      </Row>
      <h3>Keyboard</h3>
      <Row title="Shortcuts" note="Every view is a key: G grid, E Loupe, C Compare, D Edit. ⌘K finds anything else.">
        <button className="import-pick" onClick={() => openSettings('keys')}>
          Show all
        </button>
      </Row>
      <UsageRows />
    </div>
  );
}
