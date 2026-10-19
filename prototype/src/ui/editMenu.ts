// The app's Edit › Undo, Redo and Select All (app/src/main.rs) arrive here as an
// `edit-menu` event instead of going to the webview, which only knows text fields.
// In a text field they keep their native behaviour; anywhere else they replay the
// same keys the screens already handle (⌘Z, ⇧⌘Z, ⌘A), so the menu and the keyboard
// can't drift apart. `argus:edit-menu` on window is the same path for tests.
import { IN_APP } from '../data/backend';

export type EditAction = 'undo' | 'redo' | 'select-all';

const MAC = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);
const KEYS: Record<EditAction, { key: string; code: string; shift: boolean }> = {
  undo: { key: 'z', code: 'KeyZ', shift: false },
  redo: { key: 'z', code: 'KeyZ', shift: true },
  'select-all': { key: 'a', code: 'KeyA', shift: false },
};

// When the webview did get the real key (the menu didn't swallow it), the menu event
// that follows must not do it a second time.
let lastKey = { combo: '', at: 0 };
const combo = (key: string, shift: boolean) => `${key.toLowerCase()}${shift ? '+shift' : ''}`;

function editable(el: Element | null): boolean {
  if (!el) return false;
  if (el instanceof HTMLTextAreaElement) return true;
  if (el instanceof HTMLInputElement) return !['checkbox', 'radio', 'range', 'button', 'submit'].includes(el.type);
  return (el as HTMLElement).isContentEditable;
}

export function runEditMenu(action: EditAction) {
  const k = KEYS[action];
  if (lastKey.combo === combo(k.key, k.shift) && performance.now() - lastKey.at < 300) return;
  const el = document.activeElement;
  if (editable(el)) {
    document.execCommand(action === 'select-all' ? 'selectAll' : action);
    return;
  }
  (el ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key: k.key, code: k.code, shiftKey: k.shift, metaKey: MAC, ctrlKey: !MAC, bubbles: true, cancelable: true }),
  );
}

/** Follow the Edit menu. Call once at start. */
export function initEditMenu() {
  document.addEventListener(
    'keydown',
    (e) => {
      // In a text field the menu is what undoes (WebKit's text undo is the menu's job), so only keys outside one count.
      if (e.isTrusted && (MAC ? e.metaKey : e.ctrlKey) && !editable(e.target as Element)) lastKey = { combo: combo(e.key, e.shiftKey), at: performance.now() };
    },
    true,
  );
  window.addEventListener('argus:edit-menu', (e) => runEditMenu((e as CustomEvent<EditAction>).detail));
  if (IN_APP) void import('@tauri-apps/api/event').then(({ listen }) => listen<EditAction>('edit-menu', (e) => runEditMenu(e.payload)));
}
