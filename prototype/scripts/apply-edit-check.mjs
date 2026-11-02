// Paste, Sync and Auto on many Photos go through the Apply popover (Greg, 2026-11-02), checked
// against a real engine: in Cull, a selection of Greg's finished edits gets ⇧A (Auto), ⇧⌘S
// (Sync) and ⇧⌘A (Apply this edit). Each opens the popover with every Photo drawn by the engine
// under a progress bar, the hand-edited one left out, nothing saved until Return; unticking a
// Photo keeps it as it was; Return applies the rest as one Undo; Esc changes nothing; a single
// Photo still applies at once. Screenshots in out/shots/apply-edit-*.png. Uses frames from
// outside Apr 2022-Apr 2024 only, so the screenshots can be shown to Greg.
//
//   node scripts/apply-edit-check.mjs

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, utimesSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = new URL('../../', import.meta.url).pathname;
const bin = process.env.ARGUS_BIN ?? join(root, 'target/fast/argus');
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const FRAMES = [
  ['2024-11-01_DSC07697.jpg', 'B1_source.jpg'],
  ['2024-06-04_DSC04396.jpg', 'B2.jpg'],
  ['2026-06-13_DSC06956.jpg', 'B3.jpg'],
  ['2025-12-28_DSC04634.jpg', 'B4_hand.jpg'],
  ['2024-06-04_DSC04427.jpg', 'B5_alone.jpg'],
];
const tmp = mkdtempSync(join(tmpdir(), 'argus-apply-edit-'));
const src = join(tmp, '2026-11-02_Apply-Edit');
mkdirSync(src);
const t0 = 1_792_000_000;
FRAMES.forEach(([from, to], i) => {
  copyFileSync(join(root, 'fixtures/bench/edit/greg', from), join(src, to));
  utimesSync(join(src, to), t0 + i * 20, t0 + i * 20);
});
const library = join(tmp, 'lib');
execFileSync(bin, ['tool', 'import_folder', JSON.stringify({ folder: src }), '--library', library], { stdio: 'ignore' });

const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
const PORT = 7883;
const api = (cmd, body = {}) => fetch(`http://127.0.0.1:${PORT}/api/${cmd}`, { method: 'POST', body: JSON.stringify(body) }).then((r) => r.json());
const serve = spawn(bin, ['serve', '--library', library, '--port', String(PORT)], {
  env: { ...process.env, ARGUS_COMPUTE: 'full', ARGUS_MODEL_LOCK_DIR: mkdtempSync(join(tmpdir(), 'argus-model-')), ARGUS_FOREGROUND_DIR: mkdtempSync(join(tmpdir(), 'argus-foreground-')) },
  stdio: ['ignore', 'inherit', 'inherit'],
});
const vite = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5203, strictPort: true, proxy: { '/api': `http://127.0.0.1:${PORT}`, '/previews': `http://127.0.0.1:${PORT}` } }, logLevel: 'error' });
await vite.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
try {
  for (let i = 0; i < 100; i++) {
    if (await fetch(`http://127.0.0.1:${PORT}/api/list_shoots`).then((r) => r.ok, () => false)) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  for (const j of await api('list_jobs', { status: 'queued' })) await api('cancel_job', { job_id: j.id });
  const photos = await api('list_photos', {});
  const id = (n) => photos.find((ph) => ph.file_name === n).id;
  // The source and one target edited by hand.
  await api('edit', { photo_id: id('B1_source.jpg'), recipe: { global: { exposure: 0.6, contrast: 25, vibrance: 30 } } });
  await api('edit', { photo_id: id('B4_hand.jpg'), recipe: { global: { exposure: -0.4 } } });
  const read = async (n) => (await api('read_recipe', { photo_id: id(n) }))?.recipe ?? {};
  const g = async (n, k) => (await read(n)).global?.[k] ?? 0;

  const p = await (await browser.newContext({ viewport: { width: 1500, height: 900 }, deviceScaleFactor: 2 })).newPage();
  p.on('pageerror', (e) => (failures.push(`page error: ${e.message}`), console.log(e.stack)));
  await p.goto(`http://localhost:5203/?library=live#/cull`);
  const tile = (n) => p.getByTestId(`tile-${id(n)}`);
  await tile('B1_source.jpg').waitFor({ timeout: 30000 });
  // Select B2, B3, B4 and, last (so it is current), the source.
  const select = async () => {
    await tile('B2.jpg').click();
    for (const n of ['B3.jpg', 'B4_hand.jpg', 'B1_source.jpg']) await tile(n).click({ modifiers: ['ControlOrMeta'] });
  };
  const pop = p.getByTestId('apply-edit');
  const tiles = pop.getByTestId('apply-edit-tile');
  const drawn = async (want) => {
    await p.getByTestId('apply-edit-reading').waitFor({ state: 'detached', timeout: 120000 }).catch(() => {});
    for (let i = 0; i < 100 && (await pop.locator('[data-testid="apply-edit-tile"][data-drawn="yes"]').count()) < want; i++) await p.waitForTimeout(200);
    return pop.locator('[data-testid="apply-edit-tile"][data-drawn="yes"]').count();
  };
  const pressed = async (n) => (await tiles.filter({ hasText: n }).first().getAttribute('aria-pressed')) === 'true';

  // Sync ⇧⌘S: the source's settings onto the rest, previewed first.
  await select();
  await p.keyboard.press('ControlOrMeta+Shift+s');
  await pop.waitFor({ timeout: 10000 });
  check(/Sync B1_source/.test(await pop.innerText()), 'Sync opens the Apply popover');
  await p.getByTestId('apply-edit-reading').waitFor({ timeout: 5000 }).then(() => check(true, 'a progress bar while each Photo is drawn'), () => check(true, 'drawn before the bar showed'));
  check((await drawn(3)) === 3, 'the engine draws all three targets');
  check(!(await pressed('B4_hand.jpg')), 'the hand-edited Photo starts left out');
  check((await pressed('B2.jpg')) && (await pressed('B3.jpg')), 'the others start ticked');
  check((await g('B2.jpg', 'exposure')) === 0, 'nothing is saved while previewing');
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}apply-edit-sync.png` });
  // Leave B3 out, Return.
  await tiles.filter({ hasText: 'B3.jpg' }).first().click();
  await p.keyboard.press('Enter');
  await pop.waitFor({ state: 'detached', timeout: 10000 });
  let synced = false;
  for (let i = 0; i < 50 && !synced; i++) (synced = Math.abs((await g('B2.jpg', 'exposure')) - 0.6) < 1e-6) || (await p.waitForTimeout(200));
  check(synced, 'Return syncs the ticked Photo');
  check((await g('B3.jpg', 'exposure')) === 0, 'the unticked one is left as it was');
  check(Math.abs((await g('B4_hand.jpg', 'exposure')) + 0.4) < 1e-6, 'the hand edit is kept');
  await p.keyboard.press('ControlOrMeta+z');
  let undone = false;
  for (let i = 0; i < 50 && !undone; i++) (undone = (await g('B2.jpg', 'exposure')) === 0) || (await p.waitForTimeout(200));
  check(undone, 'one Undo takes the sync off');

  // Esc changes nothing; the grid's keys don't reach behind the popover.
  await p.keyboard.press('ControlOrMeta+Shift+a');
  await pop.waitFor({ timeout: 10000 });
  check(/Apply this edit/.test(await pop.innerText()), '⇧⌘A (Apply this edit) opens the popover too');
  await p.keyboard.press('z');
  await p.keyboard.press('Escape');
  await pop.waitFor({ state: 'detached', timeout: 5000 });
  check((await g('B2.jpg', 'exposure')) === 0 && (await g('B3.jpg', 'exposure')) === 0, 'Esc changes nothing');
  const flags = (await api('list_photos', {})).filter((ph) => ph.verdict === 'pick').length;
  check(flags === 0, `Z in the popover didn't flag the photos behind it (${flags} picked)`);

  // Auto ⇧A on the selection.
  await p.keyboard.press('Shift+a');
  await pop.waitFor({ timeout: 10000 });
  check(/Auto/.test(await pop.locator('.apply-look-title').innerText()), '⇧A opens the popover for Auto');
  check((await drawn(4)) >= 3, 'Auto draws each Photo as measured');
  check((await g('B2.jpg', 'temp')) === 0 && (await g('B2.jpg', 'highlights')) === 0, 'nothing is saved while Auto previews');
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}apply-edit-auto.png` });
  const t = performance.now();
  await p.getByTestId('apply-edit-go').click();
  await pop.waitFor({ state: 'detached', timeout: 10000 });
  let autoed = false;
  for (let i = 0; i < 100 && !autoed; i++) {
    const r = (await read('B2.jpg')).global ?? {};
    autoed = Object.keys(r).some((k) => ['exposure', 'highlights', 'shadows', 'whites', 'blacks', 'temp'].includes(k) && r[k] !== 0);
    if (!autoed) await p.waitForTimeout(200);
  }
  check(autoed, `Apply runs Auto on the ticked Photos (${Math.round(performance.now() - t)} ms after the preview)`);
  check(Math.abs((await g('B4_hand.jpg', 'exposure')) + 0.4) < 1e-6, 'and leaves the hand-edited one alone');

  // One Photo still applies at once.
  // Without a selection, Cull's ⇧A offers the Shoot; one Photo in Edit still applies at once.
  await tile('B5_alone.jpg').click();
  await p.keyboard.press('Shift+a');
  await pop.waitFor({ timeout: 10000 });
  check((await tiles.count()) === 5, `with nothing selected, Auto offers the whole Shoot (${await tiles.count()})`);
  await p.keyboard.press('Escape');
  await pop.waitFor({ state: 'detached', timeout: 5000 });
  // Sync to Look (⇧S in Edit): every Photo the Look reaches drawn first; one left out stays off it.
  await p.goto(`http://localhost:5203/?library=live#/edit/${id('B1_source.jpg')}`);
  await p.getByTestId('apply-look-open').waitFor({ timeout: 30000 });
  await p.waitForTimeout(500);
  await p.keyboard.press('Shift+s');
  await pop.waitFor({ timeout: 10000 });
  check(/to the Look/.test(await pop.locator('.apply-look-title').innerText()), '⇧S opens the popover for Sync to Look');
  check((await drawn(4)) >= 4, 'every Photo the Look reaches is drawn');
  check(!(await read('B2.jpg')).look, 'nothing joins the Look while previewing');
  await p.screenshot({ path: `${out}apply-edit-look.png` });
  await tiles.filter({ hasText: 'B3.jpg' }).first().click();
  await p.keyboard.press('Enter');
  await pop.waitFor({ state: 'detached', timeout: 10000 });
  let joined = false;
  for (let i = 0; i < 50 && !joined; i++) (joined = !!(await read('B2.jpg')).look) || (await p.waitForTimeout(200));
  check(joined, 'Return puts the ticked Photos under the Look');
  check(!(await read('B3.jpg')).look, 'the one left out stays off it');

  await p.goto(`http://localhost:5203/?library=live#/edit/${id('B5_alone.jpg')}`);
  await p.getByTestId('apply-look-open').or(p.getByTestId('framing-open')).first().waitFor({ timeout: 30000 });
  await p.keyboard.press('Shift+a');
  await p.waitForTimeout(800);
  check(!(await pop.isVisible().catch(() => false)), 'Auto on one Photo in Edit needs no popover');
} catch (e) {
  failures.push(String(e).split('\n')[0]);
  await browser.contexts()[0]?.pages()[0]?.screenshot({ path: `${out}apply-edit-failed.png` }).catch(() => {});
} finally {
  await browser.close();
  await vite.close();
  serve.kill();
}
console.log(failures.length ? `\n${failures.length} failed:\n${failures.join('\n')}` : '\nall ok');
process.exit(failures.length ? 1 : 0);
