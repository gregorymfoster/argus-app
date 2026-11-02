// Every main screen at Greg's window widths (1280 and 1500 px): fails when a header or
// footer wraps, or a label in one is clipped or sticks out, the Status bar included, plus the sidebar squeezed. Shots in out/shots/layout-*.png.
//
//   node scripts/layout.mjs

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
import { WIDTHS, barProblems, layoutProblems } from './layout-lib.mjs';

const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5197, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const SCREENS = { import: '#/import', grid: '?demo=culled&update=ready#/cull', loupe: '?demo=culled#/loupe/p5', compare: '?demo=culled#/compare/p4,p5', cull: '?demo=culled#/moments', export: '?demo=culled#/export', edit: '?demo=culled#/edit/p15', look: '?demo=culled#/look' };
const failures = [];
try {
  for (const w of WIDTHS)
    for (const [name, path] of Object.entries(SCREENS)) {
      const ctx = await browser.newContext({ viewport: { width: w, height: 860 } });
      const p = await ctx.newPage();
      await p.goto(`http://localhost:5197/${path}`);
      await p.waitForTimeout(900);
      const problems = [...(await barProblems(p)), ...(await layoutProblems(p))];
      await p.screenshot({ path: `${out}layout-${name}-${w}.png` });
      console.log(`${problems.length ? 'FAIL' : 'ok  '} ${name} at ${w}px${problems.length ? '\n       ' + problems.join('\n       ') : ''}`);
      if (problems.length) failures.push(`${name}@${w}`);
      await ctx.close();
    }
  // The awkward cases real Shoots have (?stress=1: portraits, a 40-frame Stack, a long Shoot
  // name), in the states that broke before: Cull showing two moments, a Stack paged, the Apply
  // look popover open over a Shoot. One width per scene keeps this under a minute.
  const STRESS = {
    'stress-grid': ['?demo=culled&stress=1#/cull'],
    'stress-loupe-portrait': ['?demo=culled&stress=1#/loupe/p2'],
    'stress-cull': ['?demo=culled&stress=1#/moments'],
    'stress-cull-next-too': ['?demo=culled&stress=1#/moments', ['Shift+ArrowDown', 'Shift+ArrowDown']],
    'stress-cull-big-stack': ['?demo=culled&stress=1#/moments', ['Tab']],
    'stress-cull-big-stack-page-2': ['?demo=culled&stress=1#/moments', ['Tab', ']']],
    'stress-cull-big-stack-next-too': ['?demo=culled&stress=1#/moments', ['Tab', 'Shift+ArrowDown']],
    'stress-compare': ['?demo=culled&stress=1#/compare/p1,p2'],
    'stress-apply-look': ['?demo=culled&stress=1#/edit/p15', ['Shift+ControlOrMeta+A'], 1500],
    'stress-export': ['?demo=culled&stress=1#/export'],
  };
  for (const w of [1024, 1500])
    for (const [name, [path, keys = []]] of Object.entries(STRESS)) {
      const ctx = await browser.newContext({ viewport: { width: w, height: 860 } });
      const p = await ctx.newPage();
      const errors = [];
      p.on('pageerror', (e) => errors.push(`page error: ${e.message}`));
      await p.goto(`http://localhost:5197/${path}`);
      await p.waitForTimeout(900);
      for (const k of keys) {
        await p.keyboard.press(k);
        await p.waitForTimeout(350);
      }
      await p.waitForTimeout(STRESS[name][2] ?? 500);
      const problems = [...errors, ...(await barProblems(p)), ...(await layoutProblems(p))];
      await p.screenshot({ path: `${out}layout-${name}-${w}.png` });
      console.log(`${problems.length ? 'FAIL' : 'ok  '} ${name} at ${w}px${problems.length ? '\n       ' + problems.join('\n       ') : ''}`);
      if (problems.length) failures.push(`${name}@${w}`);
      await ctx.close();
    }
  // A squeezed sidebar, since Greg's fonts and zoom run wider than Chromium's here.
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const p = await ctx.newPage();
  await p.goto(`http://localhost:5197/${SCREENS.grid}`);
  await p.addStyleTag({ content: 'nav.sidebar { width: 150px !important; }' });
  await p.waitForTimeout(900);
  const problems = await barProblems(p);
  await p.screenshot({ path: `${out}layout-sidebar-150.png` });
  console.log(`${problems.length ? 'FAIL' : 'ok  '} sidebar at 150px${problems.length ? '\n       ' + problems.join('\n       ') : ''}`);
  if (problems.length) failures.push('sidebar@150');
  await ctx.close();
} finally {
  await browser.close();
  await server.close();
}
console.log(failures.length ? `\n${failures.length} failed: ${failures.join(', ')}` : '\nall ok');
process.exit(failures.length ? 1 : 0);
