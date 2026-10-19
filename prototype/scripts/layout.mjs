// Every main screen at Greg's window widths (1280 and 1500 px): fails when a header or
// footer wraps, or a label in one is clipped or sticks out. Shots in out/shots/layout-*.png.
//
//   node scripts/layout.mjs

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';
import { WIDTHS, barProblems } from './layout-lib.mjs';

const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5197, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const SCREENS = { import: '#/import', grid: '?demo=culled#/cull', loupe: '?demo=culled#/loupe/p5', compare: '?demo=culled#/compare/p4,p5', edit: '?demo=culled#/edit/p15', look: '?demo=culled#/look' };
const failures = [];
try {
  for (const w of WIDTHS)
    for (const [name, path] of Object.entries(SCREENS)) {
      const ctx = await browser.newContext({ viewport: { width: w, height: 860 } });
      const p = await ctx.newPage();
      await p.goto(`http://localhost:5197/${path}`);
      await p.waitForTimeout(900);
      const problems = await barProblems(p);
      await p.screenshot({ path: `${out}layout-${name}-${w}.png` });
      console.log(`${problems.length ? 'FAIL' : 'ok  '} ${name} at ${w}px${problems.length ? '\n       ' + problems.join('\n       ') : ''}`);
      if (problems.length) failures.push(`${name}@${w}`);
      await ctx.close();
    }
} finally {
  await browser.close();
  await server.close();
}
console.log(failures.length ? `\n${failures.length} failed: ${failures.join(', ')}` : '\nall ok');
process.exit(failures.length ? 1 : 0);
