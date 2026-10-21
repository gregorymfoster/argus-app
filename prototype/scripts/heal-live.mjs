// The Heal tool on a real Library: opens Heal (H), sizes the brush with ], clicks a spot on
// the photo, checks the engine saved a Spot with a source it picked and re-rendered, drags
// the source, then removes the Spot with Delete. Screenshots before, after and a close-up.
//   node scripts/heal-live.mjs <library-dir> [photo-id] [x,y]
// x,y is where to heal, 0..1 of the uncropped photo (default the middle). The Library is
// copied first, so it is never changed.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/heal-live.mjs <library-dir> [photo-id] [x,y]');
  process.exit(2);
}
const [hx, hy] = (process.argv[4] ?? '0.5,0.5').split(',').map(Number);
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-heal-')), 'lib');
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
const spots = async () => (await api('read_recipe', { photo_id: Number(target) }))?.recipe?.spots ?? [];
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
let ok = true;
const check = (cond, what) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'} ${what}`);
  ok &&= cond;
};
const settle = async (p) => {
  await p.waitForTimeout(300);
  await p.getByTestId('rendering').waitFor({ state: 'detached', timeout: 20000 }).catch(() => {});
  await p.waitForTimeout(400);
};
try {
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 1000 }, deviceScaleFactor: 1 });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(e.message));
  await p.goto(`http://localhost:5199/?library=live#/edit/${target}`);
  await p.getByTestId('edit-panel').waitFor();
  await p.waitForTimeout(1500);
  await p.keyboard.press('h');
  await p.getByTestId('heal-panel').waitFor();
  await settle(p);
  const layer = p.getByTestId('heal-overlay');
  const b = await layer.boundingBox();
  const clip = { x: b.x + b.width * hx - 120, y: b.y + b.height * hy - 90, width: 240, height: 180 };
  await p.mouse.move(b.x + 5, b.y + 5);
  await p.screenshot({ path: `${out}heal-live-before.png` });
  await p.screenshot({ path: `${out}heal-live-close-before.png`, clip });
  for (let i = 0; i < Number(process.env.HEAL_GROW ?? 5); i++) await p.keyboard.press(']');
  await p.mouse.click(b.x + b.width * hx, b.y + b.height * hy);
  await p.getByTestId('spot-ring').waitFor();
  await settle(p);
  let saved = await spots();
  check(saved.length === 1, `one Spot saved (${saved.length})`);
  const s = saved[0] ?? {};
  check(Math.abs(s.x - hx) < 0.01 && Math.abs(s.y - hy) < 0.01, `at the click (${s.x}, ${s.y})`);
  check(Math.abs(s.r - 0.015 * 1.15 ** Number(process.env.HEAL_GROW ?? 5)) < 0.002, `sized by ] (${s.r})`);
  check(s.from && Math.hypot(s.from.x - s.x, s.from.y - s.y) > s.r, `the engine picked a source clear of it (${JSON.stringify(s.from)})`);
  await p.mouse.move(b.x + 5, b.y + 5);
  await settle(p);
  await p.screenshot({ path: `${out}heal-live-after.png` });
  // The close-up without the circles: Before is the saved state with the tool closed.
  await p.keyboard.press('e');
  await settle(p);
  await p.keyboard.press('h');
  await settle(p);
  await p.locator('.spot-ring').first().waitFor();
  await p.evaluate(() => document.querySelectorAll('.heal-layer').forEach((e) => (e.style.visibility = 'hidden')));
  await p.waitForTimeout(200);
  await p.screenshot({ path: `${out}heal-live-close-after.png`, clip });
  await p.evaluate(() => document.querySelectorAll('.heal-layer').forEach((e) => (e.style.visibility = '')));

  // Drag the source somewhere else.
  const ring = p.locator('.spot-ring.from').first();
  const rb = await ring.boundingBox();
  await p.mouse.move(rb.x + rb.width / 2, rb.y + rb.height / 2);
  await p.mouse.down();
  await p.mouse.move(rb.x + rb.width / 2 - 40, rb.y + rb.height / 2 + 10, { steps: 5 });
  await p.mouse.up();
  await settle(p);
  saved = await spots();
  check(saved.length === 1 && Math.abs(saved[0].from.x - s.from.x) > 0.01, `dragging the source moves it (${JSON.stringify(saved[0]?.from)})`);

  await p.keyboard.press('Delete');
  await settle(p);
  check((await spots()).length === 0, 'Delete removes the Spot');
  check(errors.length === 0, `no page errors ${errors.join(' | ')}`);
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
console.log(`shots in ${out}heal-live-*.png`);
process.exit(ok ? 0 : 1);
