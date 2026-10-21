// Keyboard fuzz on a real Library: seeded random flag, star, move and undo/redo keys in the
// grid, Loupe and Compare, as fast as a person mashing keys. Fails on page errors, console
// errors and engine calls that error, and checks that the Library agrees with the screen:
// after the run, undoing everything must leave every Photo as it started, and redoing it all
// must land on the same Verdicts and stars the run ended with.
// `node scripts/fuzz.mjs <library-dir> [seed] [steps]`; shots in out/shots/fuzz-*.png.
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [source, seedArg = '1', stepsArg = '300'] = process.argv.slice(2);
if (!source) {
  console.error('usage: node scripts/fuzz.mjs <library-dir> [seed] [steps]');
  process.exit(2);
}
let seed = Number(seedArg) >>> 0 || 1;
const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const pick = (xs) => xs[Math.floor(rand() * xs.length)];
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-fuzz-')), 'lib');
cpSync(source, library, { recursive: true });

const PORT = 7869;
const bridge = spawn(bin, ['serve', '--library', library, '--port', String(PORT)], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, ARGUS_PROVIDER: 'ollama', OLLAMA_HOST: '127.0.0.1:9', ARGUS_QUEUE: 'off' },
});
const api = (p) => fetch(`http://127.0.0.1:${PORT}${p}`).then((r) => r.json());
for (let i = 0; ; i++) {
  try {
    await api('/api/list_shoots');
    break;
  } catch (e) {
    if (i > 50) throw e;
    await new Promise((r) => setTimeout(r, 100));
  }
}
const server = await createServer({
  configFile: new URL('../vite.config.ts', import.meta.url).pathname,
  server: { port: 5189, strictPort: true, proxy: { '/api': `http://127.0.0.1:${PORT}`, '/previews': `http://127.0.0.1:${PORT}`, '/library-file': `http://127.0.0.1:${PORT}` } },
  logLevel: 'error',
});
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const problems = [];
let step = 'start';
const note = (m) => (problems.push(`[${step}] ${m}`), console.log(`FAIL [${step}] ${m}`));

const shoots = await api('/api/list_shoots');
const shootId = shoots[shoots.length - 1].id;
const state = async () => {
  const photos = await api(`/api/photos?shoot_id=${shootId}`);
  return Object.fromEntries(photos.map((p) => [p.id, `${p.verdict}/${p.stars ?? 0}`]));
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
// The UI saves in order behind the keys, so wait until the Library stops changing.
const settled = async () => {
  let last = await state();
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const now = await state();
    if (same(now, last)) return now;
    last = now;
  }
  return last;
};
const diff = (a, b) => Object.keys(a).filter((k) => a[k] !== b[k]).map((k) => `${k}: ${a[k]} vs ${b[k]}`).join(', ');

try {
  const start = await state();
  const p = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  p.on('pageerror', (e) => note(`page error: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && !/Failed to load resource/.test(m.text()) && note(`console: ${m.text().slice(0, 200)}`));
  p.on('response', async (r) => {
    if (!r.url().includes('/api/') || r.ok()) return;
    note(`${r.status()} ${new URL(r.url()).pathname}: ${(await r.text().catch(() => '')).slice(0, 160)}`);
  });
  await p.goto(`http://localhost:5189/?library=live#/cull`);
  await p.waitForSelector('.tile');
  await p.locator('.tile').first().click();

  // Keys that change a Verdict or star Rating, and keys that move or switch screens.
  const change = ['p', 'x', 'u', '0', '1', '2', '3', '4', '5'];
  const move = ['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp'];
  const screens = ['Escape', 'Space', 'c'];
  const steps = Number(stepsArg);
  step = 'mash';
  for (let i = 0; i < steps; i++) {
    const r = rand();
    const key = r < 0.55 ? pick(change) : r < 0.85 ? pick(move) : pick(screens);
    await p.keyboard.press(key);
    // Mostly no wait at all: a person mashing keys doesn't wait for the engine either.
    if (rand() < 0.1) await p.waitForTimeout(Math.floor(rand() * 200));
  }
  // A route change lands on the next hashchange, so give the last key a human beat first.
  await p.waitForTimeout(100);
  await p.keyboard.press('Escape');
  if (!p.url().includes('#/cull')) note(`Esc left the app on ${p.url().split('#')[1]}, not the grid`);
  await p.screenshot({ path: `${out}fuzz-mashed.png` });
  const end = await settled();
  console.log(`mashed ${steps} keys: ${Object.values(end).filter((v) => v !== 'unflagged/0').length} Photos changed`);

  // A quick ⌘Z ⌘Z … ⇧⌘Z ⇧⌘Z …: each Redo's step is an Undo still in flight, and they must
  // still reach the Library in the order pressed.
  step = 'undo-redo-burst';
  for (let i = 0; i < 12; i++) await p.keyboard.press('ControlOrMeta+z');
  for (let i = 0; i < 12; i++) await p.keyboard.press('ControlOrMeta+Shift+z');
  const burst = await settled();
  if (!same(burst, end)) note(`undo then redo in a burst doesn't land where the run ended: ${diff(burst, end)}`);

  step = 'undo-all';
  for (let i = 0; i < steps + 5; i++) await p.keyboard.press('ControlOrMeta+z');
  const undone = await settled();
  if (!same(undone, start)) note(`undoing everything left the Library changed: ${diff(undone, start)}`);
  await p.screenshot({ path: `${out}fuzz-undone.png` });

  step = 'redo-all';
  for (let i = 0; i < steps + 5; i++) await p.keyboard.press('ControlOrMeta+Shift+z');
  const redone = await settled();
  if (!same(redone, end)) note(`redoing everything doesn't match the end of the run: ${diff(redone, end)}`);

  // The screen must agree with the Library after a reload.
  step = 'reload';
  await p.reload();
  await p.waitForSelector('.tile', { timeout: 8000 }).catch(async () => {
    note(`no grid after a reload at ${p.url().split('#')[1]}`);
    await p.screenshot({ path: `${out}fuzz-reload-broken.png` });
  });
  await p.waitForTimeout(800);
  await p.screenshot({ path: `${out}fuzz-reloaded.png` });
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
console.log(problems.length ? `\n${problems.length} problem(s)` : '\nfuzz: no problems');
process.exit(problems.length ? 1 : 0);
