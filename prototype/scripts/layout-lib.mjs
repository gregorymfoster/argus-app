// Layout checks shared by the screenshot scripts: header and footer bars must stay one
// line at Greg's real window sizes, with no label wrapped or clipped (Greg, 2026-10-18,
// after headers broke at his MacBook's width). The Status bar (the meter, Background
// work, Restart to update) is checked the same way (Greg, 2026-10-20, after the update pill wrapped).

// A webview at 125% zoom lays out like a window 1/1.25 as wide: 1280 px at 125% is 1024.
export const WIDTHS = [1280, 1500, 1024];

/** Problems in the page's bars right now, as readable strings (empty when it's fine). */
export async function barProblems(p) {
  return p.evaluate(() => {
    const out = [];
    const name = (el) => (el.dataset.testid ? `[${el.dataset.testid}]` : `${el.tagName.toLowerCase()}${el.className && typeof el.className === 'string' ? '.' + el.className.split(' ').filter(Boolean).slice(0, 2).join('.') : ''}`);
    for (const bar of document.querySelectorAll('.panel-head, .panel-foot, .import-strip, .loupe-bar, .view-foot, .look-bar, .status-bar, .sidebar .nav-import')) {
      const box = bar.getBoundingClientRect();
      if (box.width === 0) continue;
      const where = `${name(bar)} "${bar.textContent.trim().slice(0, 30)}"`;
      if (box.height > 44) out.push(`${where} is ${Math.round(box.height)}px tall (wrapped)`);
      for (const el of bar.querySelectorAll('*')) {
        const r = el.getBoundingClientRect();
        if (r.width === 0 || getComputedStyle(el).display === 'none') continue;
        const text = [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => n.textContent.trim()).join(' ');
        if (r.right > box.right + 1 || r.left < box.left - 1) out.push(`${where}: ${name(el)} "${el.textContent.trim().slice(0, 30)}" sticks out of the bar`);
        if (!text) continue;
        const cs = getComputedStyle(el);
        // A bare text node in a flex item wraps without making the item taller, so count its lines.
        for (const n of el.childNodes) {
          if (n.nodeType !== 3 || !n.textContent.trim()) continue;
          const range = document.createRange();
          range.selectNodeContents(n);
          const tops = new Set([...range.getClientRects()].filter((q) => q.width > 0).map((q) => Math.round(q.top)));
          if (tops.size > 1) out.push(`${where}: "${n.textContent.trim().slice(0, 30)}" wraps onto ${tops.size} lines`);
        }
        const line = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.3;
        if (r.height > line * 1.6 && cs.display !== 'flex' && cs.display !== 'inline-flex') out.push(`${where}: "${text.slice(0, 30)}" wraps onto ${Math.round(r.height / line)} lines`);
        if (el.scrollWidth > el.clientWidth + 1 && cs.textOverflow !== 'ellipsis' && cs.overflow !== 'visible') out.push(`${where}: "${text.slice(0, 30)}" is clipped`);
      }
    }
    return [...new Set(out)];
  });
}

// Whole-page layout invariants (2026-11-02, after Cull's "Show next too" drew tiles on top of
// each other and the Apply look popover cut its progress label to "Reading eac..."). Run on
// every screenshot by shots.mjs and layout.mjs, so a broken layout fails a check instead of
// reaching Greg. Each rule names the element so the failure says where to look.
//
// 1. Photo tiles never overlap each other (grid, Cull rows, Compare, Apply look, Import, Export).
// 2. Text is never cut off: a label that overflows its box needs an ellipsis, and inside a
//    popover, dialog or menu it may not be cut at all unless it has a title
//    (hover shows the rest). File names in tiles may ellipsize.
// 3. Nothing spills out of a popover or dialog, and no popover or dialog runs off the window.
//
// An element can opt out with data-layout-ok="why" (say why: a drag ghost, an intended overlap).
export const TILES = '.tile, .apply-tile, .burst-cell, .compare-cell, .export-tile, .import-tile, .people-tile, .moment-band, .filmstrip > *';
// Where a label may not be cut short even with an ellipsis: popovers, dialogs and menus are
// sized for their content, so a cut label there is a layout bug ("Reading eac..."). The bars'
// info lines ellipsize on purpose in narrow windows; barProblems checks the bars.
export const SURFACES = '[role=dialog], [role=menu], [role=tooltip], [data-radix-popper-content-wrapper] > *';

/** Problems in the whole page right now, as readable strings (empty when it's fine). */
export async function layoutProblems(p) {
  return p.evaluate(({ TILES, SURFACES }) => {
    const out = [];
    const W = innerWidth;
    const H = innerHeight;
    const name = (el) => {
      if (el.dataset?.testid) return `[${el.dataset.testid}]`;
      const c = typeof el.className === 'string' ? el.className.split(' ').filter((x) => x && !x.includes(':') && !x.includes('[')).slice(0, 2).join('.') : '';
      return `${el.tagName.toLowerCase()}${c ? '.' + c : ''}`;
    };
    const say = (el) => `${name(el)} "${(el.textContent || el.getAttribute('aria-label') || '').trim().replace(/\s+/g, ' ').slice(0, 32)}"`;
    const ok = (el) => el.closest('[data-layout-ok]');
    // The part of an element a person can see: its box cut by the window and by every
    // ancestor that clips (a scrolled grid), or null when none of it shows.
    const shown = (el) => {
      const b = el.getBoundingClientRect();
      let r = { left: Math.max(0, b.left), top: Math.max(0, b.top), right: Math.min(W, b.right), bottom: Math.min(H, b.bottom) };
      for (let e = el; e; e = e.parentElement) {
        const cs = getComputedStyle(e);
        if (cs.display === 'none' || cs.visibility === 'hidden' || parseFloat(cs.opacity) === 0) return null;
        if (e !== el && cs.overflow !== 'visible') {
          const c = e.getBoundingClientRect();
          r = { left: Math.max(r.left, c.left), top: Math.max(r.top, c.top), right: Math.min(r.right, c.right), bottom: Math.min(r.bottom, c.bottom) };
        }
        if (cs.position === 'fixed') break;
      }
      if (r.right - r.left < 2 || r.bottom - r.top < 2) return null;
      return { ...r, width: r.right - r.left, height: r.bottom - r.top };
    };

    // 1. Tiles overlapping.
    const tiles = [...document.querySelectorAll(TILES)].filter((t) => !ok(t)).map((t) => [t, shown(t)]).filter(([, r]) => r);
    let overlaps = 0;
    for (let i = 0; i < tiles.length; i++)
      for (let j = i + 1; j < tiles.length; j++) {
        const [a, ra] = tiles[i];
        const [b, rb] = tiles[j];
        if (a.contains(b) || b.contains(a)) continue;
        const x = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
        const y = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
        if (x > 3 && y > 3 && overlaps++ < 3) out.push(`tiles overlap by ${Math.round(x)}x${Math.round(y)}px: ${say(a)} and ${say(b)}`);
      }
    if (overlaps > 3) out.push(`...and ${overlaps - 3} more overlapping tiles`);

    // 2. Cut-off text.
    for (const el of document.body.querySelectorAll('*')) {
      if (!el.childNodes.length || ok(el)) continue;
      const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
      if (!own) continue;
      const r = shown(el);
      if (!r) continue;
      const cs = getComputedStyle(el);
      if (el.scrollWidth <= el.clientWidth + 1 || cs.overflowX === 'visible' || cs.overflowX === 'auto' || cs.overflowX === 'scroll') continue;
      if (['INPUT', 'TEXTAREA', 'SELECT', 'OPTION'].includes(el.tagName)) continue;
      const surface = el.closest(SURFACES);
      if (cs.textOverflow !== 'ellipsis') out.push(`text cut off with no ellipsis: ${say(el)} needs ${el.scrollWidth}px, has ${el.clientWidth}px`);
      else if (surface && !el.closest('[title]') && !el.closest(TILES)) out.push(`text cut short in ${name(surface)}: ${say(el)} needs ${el.scrollWidth}px, has ${el.clientWidth}px (widen it, shorten it, or give it a title)`);
    }

    // 3. Popovers and dialogs: inside the window, nothing spilling out.
    for (const s of document.querySelectorAll('[role=dialog], [role=menu], [data-radix-popper-content-wrapper] > *')) {
      const r = shown(s);
      if (!r || ok(s)) continue;
      if (r.left < -1 || r.top < -1 || r.right > W + 1 || r.bottom > H + 1) out.push(`${say(s)} runs off the ${W}x${H} window`);
      const cs = getComputedStyle(s);
      if (cs.overflow !== 'visible') continue; // it clips its own content; rule 2 covers the text
      for (const el of s.querySelectorAll('*')) {
        const q = shown(el);
        if (!q || ok(el) || getComputedStyle(el).position === 'fixed') continue;
        if (q.right > r.right + 2 || q.left < r.left - 2 || q.bottom > r.bottom + 2) {
          out.push(`${say(el)} spills out of ${name(s)}`);
          break;
        }
      }
    }
    return [...new Set(out)];
  }, { TILES, SURFACES });
}
