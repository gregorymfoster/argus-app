// AI Denoise on a real Library: sets Denoise in the Edit panel's Detail section, checks the
// engine saved the Amount and queued the base, that the panel shows a progress bar while the
// Queue computes it, and that the Photo re-renders once it lands. Screenshots on the way.
//   node scripts/denoise-live.mjs <library-dir> [photo-id]
// The model is swapped for a blur (ARGUS_DENOISE_FAKE) so the run takes seconds; with
// DENOISE_REAL=1 the real model runs (minutes on a cloud CPU): the script then stops after
// the progress shot. Build a Library with `argus import <folder> --library <dir>`.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/denoise-live.mjs <library-dir> [photo-id]');
  process.exit(2);
}
const real = Boolean(process.env.DENOISE_REAL);
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-denoise-')), 'lib');
cpSync(source, library, { recursive: true });

const env = { ...process.env, ARGUS_COMPUTE: 'full' };
if (!real) env.ARGUS_DENOISE_FAKE = '1';
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
try {
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 1000 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(e.message));
  await p.goto(`http://localhost:5199/?library=live&chrome=mac#/edit/${target}`);
  await p.getByTestId('edit-panel').waitFor();
  await p.waitForTimeout(2500);
  const detail = p.locator('[data-testid=section-detail]');
  await detail.locator('.dev-section-toggle').click();
  const thumb = detail.getByRole('slider', { name: 'AI Denoise amount', exact: true });
  await thumb.scrollIntoViewIfNeeded();
  await thumb.focus();
  for (let i = 0; i < 50; i++) await p.keyboard.press('ArrowRight');
  const stage = p.locator('[data-testid=edit-stage] img').first();
  const srcBefore = await stage.getAttribute('src').catch(() => null);

  const bar = p.getByTestId('denoise-progress');
  await bar.waitFor({ timeout: 20000 }).then(
    () => check(true, 'progress bar shows while the base computes'),
    () => check(real ? false : true, 'progress bar (the fake can finish before it shows)'),
  );
  if (real) await p.waitForTimeout(60000);
  await detail.screenshot({ path: `${out}denoise-detail${real ? '-real' : ''}.png` });
  await p.screenshot({ path: `${out}denoise-edit${real ? '-real' : ''}.png` });

  const r = (await api('read_recipe', { photo_id: Number(target) })).recipe?.global ?? {};
  check(r.denoise > 0, `Denoise Amount saved (${r.denoise})`);
  if (!real) {
    let s;
    for (let i = 0; i < 120; i++) {
      s = await api('denoise_status', { photo_id: Number(target) });
      if (s.ready) break;
      await p.waitForTimeout(500);
    }
    check(s?.ready, `base computed (${JSON.stringify(s)})`);
    await bar.waitFor({ state: 'detached', timeout: 10000 }).then(
      () => check(true, 'bar gone once the base is ready'),
      () => check(false, 'bar gone once the base is ready'),
    );
    await p.waitForTimeout(4000);
    const srcAfter = await stage.getAttribute('src').catch(() => null);
    check(srcAfter && srcAfter !== srcBefore, 'Photo re-rendered with the base');
    await detail.screenshot({ path: `${out}denoise-detail-done.png` });
  }
  check(!errors.length, `no page errors ${errors.join('; ')}`);
  await ctx.close();
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
console.log(`screenshots: ${out}denoise-*.png`);
process.exit(ok ? 0 : 1);
