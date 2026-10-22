// Lens Blur on a real Library: turns it on in the Edit panel, checks the engine saved the
// Amount and the panel shows the depth histogram, switches Bokeh, focuses on a point, shows
// the depth view and paints a Refine stroke. Screenshots on the way.
//   node scripts/lens-blur-live.mjs <library-dir> [photo-id]
// The depth model is swapped for a stand-in (ARGUS_DEPTH_FAKE) unless DEPTH_REAL=1.
// Build a Library with `argus import <folder> --library <dir>`.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/lens-blur-live.mjs <library-dir> [photo-id]');
  process.exit(2);
}
const real = Boolean(process.env.DEPTH_REAL);
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-lensblur-')), 'lib');
cpSync(source, library, { recursive: true });

const env = { ...process.env, ARGUS_COMPUTE: 'full' };
if (!real) env.ARGUS_DEPTH_FAKE = '1';
const bridge = spawn(bin, ['serve', '--library', library, '--port', '7879'], { stdio: ['ignore', 'inherit', 'inherit'], env });
const api = (cmd, args = {}) => fetch(`http://127.0.0.1:7879/api/${cmd}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args) }).then((r) => r.json());
for (let i = 0; ; i++) {
  try {
    await api('list_shoots');
    break;
  } catch (e) {
    if (i > 50) throw e;
    await new Promise((r) => setTimeout(r, 200));
  }
}
const target = process.argv[3] ?? String((await api('photos'))[0].id);
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7879', '/previews': 'http://127.0.0.1:7879' } }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
let ok = true;
const check = (cond, what) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'} ${what}`);
  ok &&= cond;
};
const lens = async () => (await api('read_recipe', { photo_id: Number(target) })).recipe?.lensBlur ?? {};
try {
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 1000 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(e.message));
  await p.goto(`http://localhost:5199/?library=live&chrome=mac#/edit/${target}`);
  await p.getByTestId('edit-panel').waitFor();
  await p.waitForTimeout(2500);
  const section = p.locator('[data-testid=section-lensblur]');
  await section.locator('.dev-section-toggle').click();
  const thumb = section.getByRole('slider', { name: 'Lens Blur amount', exact: true });
  await thumb.scrollIntoViewIfNeeded();
  await thumb.focus();
  for (let i = 0; i < 70; i++) await p.keyboard.press('ArrowRight');
  await p.waitForTimeout(1500);
  check((await lens()).amount > 0, `Amount saved (${(await lens()).amount})`);

  await section.getByTestId('lens-blur').waitFor();
  await p.locator('.depth-hist').first().waitFor({ timeout: 30000 }).then(
    () => check(true, 'depth histogram shows'),
    () => check(false, 'depth histogram shows'),
  );
  await section.getByRole('radio', { name: /five-blade/ }).click();
  await p.waitForTimeout(1500);
  check((await lens()).bokeh === 'blade', 'Bokeh switched to Blade');
  await p.waitForTimeout(3000);
  await section.scrollIntoViewIfNeeded();
  await section.screenshot({ path: `${out}lens-blur-panel.png` });
  await p.screenshot({ path: `${out}lens-blur-edit.png` });

  // Focus on a point: F, then click near the top of the photo (the far background).
  const stage = p.getByTestId('edit-stage');
  const box = await stage.locator('img').first().boundingBox();
  await section.getByTestId('lens-blur-point').click();
  await p.mouse.click(box.x + box.width * 0.5, box.y + box.height * 0.08);
  await p.waitForTimeout(2000);
  const f = (await lens()).focus;
  check(Array.isArray(f) && f.length === 2, `focus set from a point (${JSON.stringify(f)})`);
  await p.waitForTimeout(2500);
  await p.screenshot({ path: `${out}lens-blur-point.png` });

  await p.keyboard.press('Shift+Z');
  await p.locator('img.depth-view').waitFor({ timeout: 15000 }).then(
    () => check(true, 'depth view shows on ⇧Z'),
    () => check(false, 'depth view shows on ⇧Z'),
  );
  await p.waitForTimeout(800);
  await p.screenshot({ path: `${out}lens-blur-depth.png` });
  await p.keyboard.press('Shift+Z');

  await section.getByRole('radio', { name: 'Paint what should stay sharp' }).click();
  await p.waitForTimeout(500);
  const b2 = await stage.locator('img').first().boundingBox();
  await p.mouse.move(b2.x + b2.width * 0.3, b2.y + b2.height * 0.2);
  await p.mouse.down();
  for (let i = 1; i <= 10; i++) await p.mouse.move(b2.x + b2.width * (0.3 + i * 0.02), b2.y + b2.height * 0.2);
  await p.mouse.up();
  await p.waitForTimeout(1500);
  check(((await lens()).focusBrush?.length ?? 0) > 0, `Refine Focus stroke saved (${(await lens()).focusBrush?.length ?? 0} dots)`);
  await p.screenshot({ path: `${out}lens-blur-refine.png` });

  check(!errors.length, `no page errors ${errors.join('; ')}`);
  await ctx.close();
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
console.log(`screenshots: ${out}lens-blur-*.png`);
process.exit(ok ? 0 : 1);
