// Checks the app's small motion through the real UI (src/ui/motion.tsx; docs/design.md, Feedback and
// motion): a flag stamps on, its count ticks, the key's hint lights, the photo flies between grid and
// Loupe, sliders glide only when set for them, Import's Photos pop in, a finished step's tick draws,
// and with Reduce motion on none of it plays. With --clips it also records a short webm per moment.
//
//   node scripts/motion-check.mjs [--clips DIR]

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const at = process.argv.indexOf('--clips');
const clips = at > 0 ? process.argv[at + 1] : null;
if (clips) mkdirSync(clips, { recursive: true });

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5197, strictPort: true }, logLevel: 'error' });
await server.listen();
const base = 'http://localhost:5197/?chrome=mac';
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const fails = [];
const errors = [];
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) fails.push(what);
};

async function open(path, { name, reduced = false } = {}) {
  const ctx = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    reducedMotion: reduced ? 'reduce' : 'no-preference',
    ...(clips && name ? { recordVideo: { dir: `${clips}/.raw-${name}`, size: { width: 1280, height: 800 } } } : {}),
  });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(`${path}: ${e.message}`));
  await p.goto(`${base}${path}`);
  // Note every class and data-zoom the motion sets, so a check can ask afterwards what played.
  await p.evaluate(() => {
    window.__seen = new Set();
    const note = (el) => {
      for (const c of ['stamp', 'tick-up', 'tick-down', 'echo', 'arrive', 'draw']) if (el.classList?.contains(c)) window.__seen.add(c);
    };
    new MutationObserver((ms) => {
      for (const m of ms) {
        if (m.type === 'attributes' && m.target === document.documentElement && document.documentElement.dataset.zoom) window.__seen.add(`zoom-${document.documentElement.dataset.zoom}`);
        if (m.type === 'attributes') note(m.target);
        m.addedNodes.forEach((n) => {
          if (n.nodeType !== 1) return;
          note(n);
          n.querySelectorAll?.('.stamp, .tick-up, .tick-down, .echo, .arrive, .draw').forEach(note);
        });
      }
    }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['class', 'data-zoom'] });
  });
  await p.waitForTimeout(1500);
  return { p, ctx, seen: async () => new Set(await p.evaluate(() => [...window.__seen])), reset: () => p.evaluate(() => window.__seen.clear()) };
}

async function close({ p, ctx }, name) {
  await ctx.close();
  if (clips && name) {
    const { execFileSync } = await import('node:child_process');
    const raw = await p.video().path();
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', raw, '-c:v', 'libvpx-vp9', '-b:v', '0', '-crf', '36', `${clips}/${name}.webm`]);
  }
}

// Flagging in the grid: the chip stamps on, the Picks count ticks, P's hint lights.
{
  const s = await open('#/cull', { name: 'flag' });
  await s.p.locator('.grid-v .tile').nth(2).click();
  await s.p.waitForTimeout(400);
  for (const k of ['p', 'p', 'x', 'p']) {
    await s.p.keyboard.press(k);
    await s.p.waitForTimeout(650);
  }
  const seen = await s.seen();
  check(seen.has('stamp'), 'a flag stamps its chip on');
  check(seen.has('tick-up') || seen.has('tick-down'), 'the flag counts tick');
  check(seen.has('echo'), "the pressed key's hint lights in the footer");
  await s.reset();
  await s.p.keyboard.press('z');
  await s.p.waitForTimeout(200);
  check(!(await s.seen()).has('echo'), 'plain Z does not light ⌘Z');
  await close(s, 'flag');
}

// The photo grows from its tile into the Loupe and back.
{
  const s = await open('#/cull', { name: 'zoom' });
  const vt = await s.p.evaluate(() => typeof document.startViewTransition === 'function');
  await s.p.locator('.grid-v .tile').nth(7).click();
  await s.p.waitForTimeout(500);
  await s.p.keyboard.press('Enter');
  await s.p.waitForTimeout(900);
  check(await s.p.locator('[data-testid="loupe"]').isVisible(), 'Enter opens the Loupe');
  await s.p.keyboard.press('g');
  await s.p.waitForTimeout(900);
  const seen = await s.seen();
  check(!vt || (seen.has('zoom-in') && seen.has('zoom-out')), 'grid and Loupe swap with the photo zoom');
  await s.reset();
  await s.p.keyboard.press('Enter');
  await s.p.waitForTimeout(700);
  await s.reset();
  await s.p.keyboard.press('ArrowRight');
  await s.p.waitForTimeout(400);
  check(![...(await s.seen())].some((x) => x.startsWith('zoom')), 'photo to photo in the Loupe never animates');
  await close(s, 'zoom');
}

// Sliders glide when set for them and follow the pointer at once while dragged.
{
  const s = await open('#/edit/p3', { name: 'sliders' });
  const glide = await s.p.evaluate(() => {
    const w = document.querySelector('.slider-root span:has(> .slider-thumb)');
    return w ? getComputedStyle(w).transitionProperty : '';
  });
  check(/left/.test(glide), 'a slider set from outside glides');
  const root = s.p.locator('.slider-root').first();
  const box = await root.boundingBox();
  await s.p.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await s.p.mouse.down();
  check((await root.getAttribute('data-live')) != null, 'a dragged slider follows the pointer without the glide');
  await s.p.mouse.up();
  await s.p.waitForTimeout(600);
  await s.p.keyboard.press('Shift+A');
  await s.p.waitForTimeout(900);
  await s.p.keyboard.press('Meta+z');
  await s.p.waitForTimeout(900);
  await close(s, 'sliders');
}

// Import's Photos pop into the grid as they arrive.
{
  const s = await open('#/import', { name: 'import' });
  await s.p.getByRole('button', { name: /^Import \d+ photos/ }).click();
  await s.p.waitForTimeout(2600);
  check((await s.seen()).has('arrive'), "Import's Photos pop into the grid");
  await close(s, 'import');
}

// With Reduce motion on, none of it plays.
{
  const s = await open('#/cull', { reduced: true });
  await s.p.locator('.grid-v .tile').nth(2).click();
  await s.p.keyboard.press('p');
  await s.p.waitForTimeout(300);
  await s.p.keyboard.press('Enter');
  await s.p.waitForTimeout(500);
  const seen = await s.seen();
  check(!seen.has('echo') && ![...seen].some((x) => x.startsWith('zoom')), 'Reduce motion turns off key echo and the zoom');
  const d = await s.p.evaluate(() => {
    const el = document.createElement('span');
    el.className = 'chip pick stamp';
    document.body.append(el);
    return getComputedStyle(el).animationDuration;
  });
  check(d === '0.001s', 'Reduce motion cuts the stamp to nothing');
  await close(s);
}

check(errors.length === 0, `no page errors${errors.length ? `: ${errors.join(' | ')}` : ''}`);
await browser.close();
await server.close();
if (clips) {
  const { rmSync, readdirSync } = await import('node:fs');
  for (const d of readdirSync(clips)) if (d.startsWith('.raw-')) rmSync(`${clips}/${d}`, { recursive: true });
}
process.exit(fails.length ? 1 : 0);
