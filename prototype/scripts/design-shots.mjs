// Retina shots of the sidebar, the Cull toolbar and the whole window, for design before/after
// comparisons (thread "Modernize the UI design language").
//
//   node scripts/design-shots.mjs OUTDIR [path]
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const out = process.argv[2] ?? 'out/design';
const path = process.argv[3] ?? '?chrome=mac#/cull';
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const p = await ctx.newPage();
await p.goto(`http://localhost:5198/${path}`);
await p.waitForTimeout(4000);
await p.screenshot({ path: `${out}/window.png` });
const side = (await p.locator('nav.sidebar').count()) ? await p.locator('nav.sidebar').boundingBox() : null;
if (side) await p.screenshot({ path: `${out}/sidebar.png`, clip: { x: 0, y: 0, width: side.width + 60, height: 900 } });
const head = (await p.locator('.cull-head').count()) ? await p.locator('.cull-head').boundingBox() : null;
if (head) await p.screenshot({ path: `${out}/toolbar.png`, clip: { x: head.x, y: 0, width: head.width, height: head.height + 70 } });
await browser.close();
await server.close();
