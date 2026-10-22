// Native Mac chrome for the desktop app: the window has an overlay title bar (traffic lights over the
// sidebar) and a translucent sidebar material (app/tauri.conf.json). Here we mark the page so CSS leaves
// the chrome transparent, and let the empty parts of headers drag the window as a title bar would.
// `?chrome=mac` previews the layout in a browser (no real translucency there) for the screenshot loop.
import { IN_APP } from '../data/backend';

const PREVIEW = new URLSearchParams(location.search).get('chrome') === 'mac';
const MAC_CHROME = PREVIEW || (IN_APP && /Mac/.test(navigator.platform));

const DRAG = '.panel-head, .sidebar-top';
const INTERACTIVE = 'button, a, input, textarea, select, [role="button"], [role="group"], [role="tab"], kbd';

export function installMacChrome() {
  if (!MAC_CHROME) return;
  document.documentElement.classList.add('mac-app');
  if (PREVIEW) document.documentElement.classList.add('mac-preview');
  if (!IN_APP) return;
  document.addEventListener('mousedown', (e) => {
    const t = e.target as HTMLElement;
    if (e.button !== 0 || !t.closest(DRAG) || t.closest(INTERACTIVE)) return;
    void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      const w = getCurrentWindow();
      return e.detail === 2 ? w.toggleMaximize() : w.startDragging();
    });
  });
}
