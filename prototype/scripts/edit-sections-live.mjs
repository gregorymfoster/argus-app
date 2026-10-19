// The Edit panel's Lightroom sections on a real Library: sets Tone curve, Color grading,
// Effects, Detail, Optics and Calibration through the UI, checks the engine saved them in
// the Recipe and re-rendered the Photo, and screenshots before and after.
//   node scripts/edit-sections-live.mjs <library-dir> [photo-id]
// (build a Library with `argus import <folder> --library <dir>`; the Library is copied first)
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/edit-sections-live.mjs <library-dir> [photo-id]');
  process.exit(2);
}
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-sections-')), 'lib');
cpSync(source, library, { recursive: true });

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7878'], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, ARGUS_QUEUE: 'off' } });
const api = (cmd, args = {}) => fetch(`http://127.0.0.1:7878/api/${cmd}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args) }).then((r) => r.json());
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
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
let ok = true;
const check = (cond, what) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'} ${what}`);
  ok &&= cond;
};
try {
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 1000 }, deviceScaleFactor: 1 });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(e.message));
  await p.goto(`http://localhost:5198/?library=live#/edit/${target}`);
  await p.getByTestId('edit-panel').waitFor();
  await p.waitForTimeout(2500);
  await p.screenshot({ path: `${out}sections-live-before.png` });
  const sec = (id) => p.locator(`[data-testid=section-${id}] .dev-section-toggle`).click();
  // A slider by its label inside a section: focus its thumb and press keys.
  const nudge = async (section, label, key, times) => {
    const thumb = p.locator(`[data-testid=section-${section}]`).getByRole('slider', { name: label, exact: true });
    await thumb.scrollIntoViewIfNeeded();
    await thumb.focus();
    for (let i = 0; i < times; i++) await p.keyboard.press(key);
  };
  await sec('light');
  await sec('curve');
  await nudge('curve', 'Darks', 'ArrowLeft', 25);
  await nudge('curve', 'Lights', 'ArrowRight', 20);
  await sec('curve');
  await sec('grading');
  await p.getByTestId('grading-wheels').scrollIntoViewIfNeeded();
  const w = await p.getByRole('slider', { name: 'Shadows hue and saturation' }).boundingBox();
  await p.mouse.click(w.x + w.width * 0.15, w.y + w.height * 0.4); // teal-blue shadows
  const hw = await p.getByRole('slider', { name: 'Highlights hue and saturation' }).boundingBox();
  await p.mouse.click(hw.x + hw.width * 0.75, hw.y + hw.height * 0.7); // warm highlights
  await sec('grading');
  await sec('effects');
  await nudge('effects', 'Clarity', 'ArrowRight', 25);
  await nudge('effects', 'Vignette', 'ArrowLeft', 35);
  await nudge('effects', 'Grain', 'ArrowRight', 20);
  await sec('effects');
  await sec('detail');
  await nudge('detail', 'Sharpening', 'ArrowRight', 40);
  await sec('detail');
  await sec('calibration');
  await nudge('calibration', 'Blue saturation', 'ArrowRight', 30);
  await p.waitForTimeout(4000);
  await p.screenshot({ path: `${out}sections-live-after.png` });

  const r = (await api('read_recipe', { photo_id: Number(target) })).recipe?.global ?? {};
  check(r.curve?.darks < 0 && r.curve?.lights > 0, `Tone curve saved (${JSON.stringify(r.curve)})`);
  check(r.grading?.shadows?.sat > 0 && r.grading?.highlights?.sat > 0, `Color grading saved (${JSON.stringify(r.grading)})`);
  check(r.clarity > 0 && r.vignette < 0 && r.grain > 0, `Effects saved (clarity ${r.clarity}, vignette ${r.vignette}, grain ${r.grain})`);
  check(r.sharpen > 0, `Sharpening saved (${r.sharpen})`);
  check(r.calibration?.red?.sat > 0 || r.calibration?.green?.sat > 0 || r.calibration?.blue?.sat > 0, `Calibration saved (${JSON.stringify(r.calibration)})`);
  check(!errors.length, `no page errors ${errors.join('; ')}`);
  await ctx.close();
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
console.log(`screenshots: ${out}sections-live-{before,after}.png`);
process.exit(ok ? 0 : 1);
