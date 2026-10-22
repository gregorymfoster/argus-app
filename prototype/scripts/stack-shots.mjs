// Shots of Stacks in the grid (collapsed, expanded, a frame's tooltip) and the Loupe's filmstrip, for by-eye checks.
//
//   node scripts/stack-shots.mjs OUTDIR
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const out = process.argv[2] ?? 'out/stacks';
const variants = ['stacks'];
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
for (const v of variants) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  await p.goto(`http://localhost:5199/?chrome=mac#/cull`);
  await p.waitForTimeout(3000);
  const grid = await p.locator('.grid-v').boundingBox();
  const clip = { x: grid.x, y: grid.y, width: grid.width, height: Math.min(grid.height, 520) };
  await p.screenshot({ path: `${out}/${v}-collapsed.png`, clip });
  // Expand the first Stack on screen from its badge, then hover a frame's badge for its tooltip.
  await p.locator('[data-testid=stack-badge]').first().click();
  await p.mouse.move(0, 0);
  await p.waitForTimeout(600);
  await p.screenshot({ path: `${out}/${v}-expanded.png`, clip });
  const n = await p.locator('[data-testid=stack-badge]').count();
  console.log(v, 'badges after expand', n);
  await p.locator('[data-testid=stack-badge]').nth(1).hover();
  await p.waitForTimeout(900);
  await p.screenshot({ path: `${out}/${v}-tooltip.png`, clip });
  // The Loupe's filmstrip: every Stack's frames on one band, the open Photo's Stack underlined.
  await p.locator('.tile.stk-cover').first().dblclick();
  await p.waitForTimeout(1200);
  const strip = await p.locator('.filmstrip').boundingBox();
  await p.screenshot({ path: `${out}/${v}-filmstrip.png`, clip: strip });
  await ctx.close();
}
await browser.close();
await server.close();
