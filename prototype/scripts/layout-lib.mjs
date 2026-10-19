// Layout checks shared by the screenshot scripts: header and footer bars must stay one
// line at Greg's real window sizes, with no label wrapped or clipped (Greg, 2026-10-18,
// after headers broke at his MacBook's width).

// A webview at 125% zoom lays out like a window 1/1.25 as wide: 1280 px at 125% is 1024.
export const WIDTHS = [1280, 1500, 1024];

/** Problems in the page's bars right now, as readable strings (empty when it's fine). */
export async function barProblems(p) {
  return p.evaluate(() => {
    const out = [];
    const name = (el) => (el.dataset.testid ? `[${el.dataset.testid}]` : `${el.tagName.toLowerCase()}${el.className && typeof el.className === 'string' ? '.' + el.className.split(' ').filter(Boolean).slice(0, 2).join('.') : ''}`);
    for (const bar of document.querySelectorAll('.panel-head, .panel-foot, .import-strip, .loupe-bar, .view-foot, .look-bar')) {
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
        const line = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.3;
        if (r.height > line * 1.6 && cs.display !== 'flex' && cs.display !== 'inline-flex') out.push(`${where}: "${text.slice(0, 30)}" wraps onto ${Math.round(r.height / line)} lines`);
        if (el.scrollWidth > el.clientWidth + 1 && cs.textOverflow !== 'ellipsis' && cs.overflow !== 'visible') out.push(`${where}: "${text.slice(0, 30)}" is clipped`);
      }
    }
    return [...new Set(out)];
  });
}
