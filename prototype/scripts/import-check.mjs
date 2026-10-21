// Import dialog check against a real engine: a fake memory card of sample RAWs, a mock
// local model that names the Shoot, then the source and New/All menus, the time and
// space line, and the grid filling while the Import runs. Screenshots in out/shots/import-*.png.
//
//   node scripts/import-check.mjs [raw-folder]   (default ../fixtures/raw-a7rv, else ../fixtures/raw)

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { tmpdir } from 'node:os';
import { WIDTHS, barProblems } from './layout-lib.mjs';
import { join } from 'node:path';

const root = new URL('../../', import.meta.url).pathname;
const rawDir = process.argv[2] ?? [join(root, 'fixtures/raw-a7rv'), join(root, 'fixtures/raw')].find((d) => existsSync(d) && readdirSync(d).length);
const bin = process.env.ARGUS_BIN ?? join(root, 'target/fast/argus');
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const tmp = mkdtempSync(join(tmpdir(), 'argus-import-'));
const volumes = join(tmp, 'Volumes');
const dcim = join(volumes, 'Untitled', 'DCIM', '100MSDCF');
mkdirSync(dcim, { recursive: true });
const raws = readdirSync(rawDir).filter((f) => /\.arw$/i.test(f));
for (const f of raws) copyFileSync(join(rawDir, f), join(dcim, f));
const library = join(tmp, 'lib');

let namingCalls = 0;
const mock = httpServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    res.setHeader('content-type', 'application/json');
    // GETs with no body (model lists, /api/ps) get an empty answer rather than crashing the mock.
    if (!body) return res.end(JSON.stringify({ models: [] }));
    const r = JSON.parse(body);
    if (r.messages.length === 0) return res.end(JSON.stringify({ message: { content: '' }, done: true }));
    namingCalls++;
    setTimeout(() => res.end(JSON.stringify({ message: { content: '{"name":"Boat Tour"}' }, done: true })), 800);
  });
});
await new Promise((r) => mock.listen(0, r));

const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
// The mock model needs no turn at the real one: a private model lock and Foreground markers,
// so a cull Greg's app is running neither stalls this check nor holds its Queue (2026-10-20).
const modelLockDir = mkdtempSync(join(tmpdir(), 'argus-model-'));
const foregroundDir = mkdtempSync(join(tmpdir(), 'argus-foreground-'));
const serve = spawn(bin, ['serve', '--library', library, '--port', '7878'], {
  env: { ...process.env, ARGUS_VOLUMES: volumes, OLLAMA_HOST: `127.0.0.1:${mock.address().port}`, ARGUS_PROVIDER: 'ollama', ARGUS_COMPUTE: 'full', ARGUS_MODEL_LOCK_DIR: modelLockDir, ARGUS_FOREGROUND_DIR: foregroundDir },
  stdio: ['ignore', 'inherit', 'inherit'],
});
const vite = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await vite.listen();
const browser = await chromium.launch();
try {
  for (let i = 0; i < 100; i++) {
    if (await fetch('http://127.0.0.1:7878/api/list_shoots').then((r) => r.ok, () => false)) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  // Keep the queue (the cull asked for below) paused: its chip must still fit the headers.
  await fetch('http://127.0.0.1:7878/api/set_queue', { method: 'POST', body: JSON.stringify({ paused: true }) });
  const p = await (await browser.newContext({ viewport: { width: 1280, height: 860 }, deviceScaleFactor: 2 })).newPage();
  p.on('pageerror', (e) => failures.push(`page error: ${e.message}`));
  await p.goto('http://localhost:5198/?library=live#/import');
  await p.getByTestId('import-tile').first().waitFor({ timeout: 30000 });
  check((await p.getByTestId('import-source').innerText()).includes(`${raws.length} photos`), 'the source line names the card and its photo count');
  for (let i = 0; i < 50 && !/Boat-Tour/.test(await p.getByTestId('import-name').inputValue()); i++) await p.waitForTimeout(200);
  const name = await p.getByTestId('import-name').inputValue();
  check(/^\d{4}-\d{2}-\d{2}_Boat-Tour$/.test(name) && namingCalls >= 1, `the local model names the Shoot (${name})`);
  await p.waitForTimeout(1500);
  const space = await p.getByTestId('import-space').innerText();
  check(new RegExp(`${raws.length} photos · .+ · (about|a few).+\\s.+ free`).test(space), `time and space before starting ("${space}")`);
  await p.screenshot({ path: `${out}import-dialog.png` });
  await p.getByTestId('import-source').click();
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}import-source-menu.png` });
  await p.keyboard.press('Escape');
  await p.waitForTimeout(300);
  await p.getByTestId('import-show').click();
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}import-show-menu.png` });
  await p.keyboard.press('Escape');
  check(await p.getByTestId('import-dialog').isVisible(), 'Esc closes a menu, not the dialog');
  await p.getByTestId('import-uncheck-all').click();
  check(/Import 0\b/.test(await p.getByTestId('import-start').innerText()), 'Untick all unticks them');
  await p.getByTestId('import-check-all').click();
  await p.getByTestId('import-auto-tone').click();
  await p.getByTestId('import-start').click();
  // The Shoot opens while the engine is still importing (a few sample RAWs go fast, so
  // watch the engine's task and the page together).
  let early = false;
  for (const t0 = Date.now(); Date.now() - t0 < 60000; ) {
    const running = (await fetch('http://127.0.0.1:7878/api/progress').then((r) => r.json())).tasks.some((t) => t.kind === 'import');
    if (p.url().includes('#/cull') && running) {
      early = true;
      await p.screenshot({ path: `${out}import-filling.png` }).catch(() => {});
      break;
    }
    if (!running && p.url().includes('#/cull')) break;
    await new Promise((r) => setTimeout(r, 100));
  }
  check(early, 'the grid opens while the Import still runs');
  await p.getByTestId('import-strip').waitFor({ state: 'detached', timeout: 600000 });
  await p.waitForTimeout(800);
  for (const t0 = Date.now(); Date.now() - t0 < 10000 && (await p.locator('.tile').count()) < raws.length; ) await p.waitForTimeout(200);
  check((await p.locator('.tile').count()) === raws.length, `every Photo lands in the grid (${await p.locator('.tile').count()})`);
  await p.screenshot({ path: `${out}import-done.png` });
  // Auto runs after the last Photo lands, before the Import reports back.
  for (const t0 = Date.now(); Date.now() - t0 < 120000 && (await fetch('http://127.0.0.1:7878/api/import_result').then((r) => r.json())).running; ) await p.waitForTimeout(300);
  const recipes = await Promise.all((await fetch('http://127.0.0.1:7878/api/photos').then((r) => r.json())).map((ph) => fetch('http://127.0.0.1:7878/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: ph.id }) }).then((r) => r.json())));
    const toned = recipes.filter((r) => r?.recipe && Object.values(r.recipe.global ?? {}).some((v) => v)).length;
  check(toned === raws.length, `Auto tone set every new Photo's sliders (${toned} of ${raws.length})`);
  // The real Library's long camera lines and the queue chip, at Greg's window widths.
  const first = (await fetch('http://127.0.0.1:7878/api/photos').then((r) => r.json()))[0].id;
  for (const w of WIDTHS)
    for (const [screen, hash] of Object.entries({ grid: '#/cull', loupe: `#/loupe/${first}`, edit: `#/edit/${first}`, look: '#/look' })) {
      await p.setViewportSize({ width: w, height: 860 });
      await p.goto(`http://localhost:5198/?library=live${hash}`);
      await p.waitForTimeout(1500);
      const problems = await barProblems(p);
      await p.screenshot({ path: `${out}import-layout-${screen}-${w}.png` });
      check(problems.length === 0, `${screen} bars fit at ${w}px${problems.length ? ': ' + problems.join('; ') : ''}`);
    }
  // Again: nothing new, so New photos is empty and says so.
  await p.goto('http://localhost:5198/?library=live#/import');
  await p.getByTestId('import-none-new').waitFor({ timeout: 30000 });
  check((await p.getByTestId('import-tile').count()) === 0, 'New photos hides what is already imported');
  await p.getByTestId('import-show').click();
  await p.getByTestId('import-show-all').click();
  check((await p.getByTestId('import-tile').count()) === raws.length, 'All photos shows them again');
} catch (e) {
  failures.push(String(e).split('\n')[0]);
  await browser.contexts()[0]?.pages()[0]?.screenshot({ path: `${out}import-failed.png` }).catch(() => {});
} finally {
  await browser.close();
  await vite.close();
  serve.kill();
  mock.close();
}
console.log(failures.length ? `\n${failures.length} failed:\n${failures.join("\n")}` : "\nall ok");
process.exit(failures.length ? 1 : 0);
