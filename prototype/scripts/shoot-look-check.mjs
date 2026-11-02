// Shoot look check against a real engine: a Shoot of sample RAWs, a hand edit on the hero in
// Edit bringing up the Apply look offer, the popover drawing every frame matched to its own
// light, one frame left out, Apply (↵) writing the rest as one Undo, and ⌘Z taking it off.
// Then Crop like this in the grid.
// Screenshots in out/shots/shoot-look-*.png.
//
//   node scripts/shoot-look-check.mjs [raw-folder]   (default ../fixtures/raw-a7rv, else ../fixtures/raw)

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = new URL('../../', import.meta.url).pathname;
const rawDir = process.argv[2] ?? [join(root, 'fixtures/raw-a7rv'), join(root, 'fixtures/raw')].find((d) => existsSync(d) && readdirSync(d).length);
const bin = process.env.ARGUS_BIN ?? join(root, 'target/fast/argus');
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const tmp = mkdtempSync(join(tmpdir(), 'argus-look-'));
const src = join(tmp, '2026-10-25_Harbour');
mkdirSync(src);
const raws = readdirSync(rawDir).filter((f) => /\.arw$/i.test(f));
for (const f of raws) copyFileSync(join(rawDir, f), join(src, f));
const library = join(tmp, 'lib');
execFileSync(bin, ['tool', 'import_folder', JSON.stringify({ folder: src }), '--library', library], { stdio: 'ignore' });

const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
const api = (cmd, body = {}) => fetch(`http://127.0.0.1:7883/api/${cmd}`, { method: 'POST', body: JSON.stringify(body) }).then((r) => r.json());
const serve = spawn(bin, ['serve', '--library', library, '--port', '7883'], {
  env: { ...process.env, ARGUS_COMPUTE: 'full', ARGUS_MODEL_LOCK_DIR: mkdtempSync(join(tmpdir(), 'argus-model-')), ARGUS_FOREGROUND_DIR: mkdtempSync(join(tmpdir(), 'argus-foreground-')) },
  stdio: ['ignore', 'inherit', 'inherit'],
});
// A crash must not leave the engine holding its port for the next run ("Address already in use").
process.on('exit', () => serve.kill());
const vite = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5203, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7883', '/previews': 'http://127.0.0.1:7883' } }, logLevel: 'error' });
await vite.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
try {
  for (let i = 0; i < 100; i++) {
    if (await fetch('http://127.0.0.1:7883/api/list_shoots').then((r) => r.ok, () => false)) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  // No AI work in the way: Apply look needs no model.
  for (const j of await api('list_jobs', { status: 'queued' })) await api('cancel_job', { job_id: j.id });
  const photos = await api('list_photos', {});
  const hero = photos[0].id;
  const p = await (await browser.newContext({ viewport: { width: 1500, height: 900 }, deviceScaleFactor: 2 })).newPage();
  p.on('pageerror', (e) => failures.push(`page error: ${e.message}`));
  await p.goto(`http://localhost:5203/?library=live#/edit/${hero}`);
  await p.getByTestId('apply-look-open').waitFor({ timeout: 30000 });

  // A hand edit on the hero brings up the quiet offer in the Edit panel.
  const temp = p.getByRole('slider', { name: /^Temp/ }).first();
  await temp.focus();
  for (let i = 0; i < 4; i++) await p.keyboard.press('Shift+ArrowRight');
  await p.getByTestId('apply-look-offer').waitFor({ timeout: 10000 });
  check(true, 'a hand edit brings up the Apply look offer');
  await p.screenshot({ path: `${out}shoot-look-offer.png` });
  await p.getByTestId('apply-look-offer-open').click();

  // The popover: every frame of the Shoot (no Picks yet, so all but Rejects), each drawn
  // matched to its own light.
  const tiles = p.getByTestId('apply-look-tile');
  await tiles.first().waitFor({ timeout: 10000 });
  check((await tiles.count()) === raws.length, `the popover shows the whole Shoot (${await tiles.count()} of ${raws.length})`);
  const drawn = () => p.locator('[data-testid=apply-look-tile][data-drawn=yes]').count();
  const t0 = Date.now();
  for (; Date.now() - t0 < 120000 && (await drawn()) < raws.length; ) await p.waitForTimeout(250);
  check((await drawn()) === raws.length, `every frame is drawn matched to the hero, in ${((Date.now() - t0) / 1000).toFixed(1)} s the first time`);
  await p.waitForTimeout(400);
  await p.screenshot({ path: `${out}shoot-look-popover.png` });

  // Leave one frame out, then Apply (↵): the rest get the look as one Undo.
  const leftOut = tiles.nth(1);
  const leftName = (await leftOut.locator('.apply-cap').innerText()).trim().replace(/\.\w+$/, '');
  await leftOut.click();
  const go = await p.getByTestId('apply-look-go').innerText();
  check(new RegExp(`Apply to ${raws.length - 2}`).test(go), `unticking a frame leaves it out (${go.split('\n')[0]})`);
  await p.keyboard.press('Enter');
  await p.getByTestId('apply-look').waitFor({ state: 'detached', timeout: 30000 });
  const recipes = async () => Object.fromEntries(await Promise.all(photos.map(async (ph) => [ph.file_name.replace(/\.\w+$/, ''), await api('read_recipe', { photo_id: ph.id })])));
  let r = await recipes();
  const ai = Object.entries(r).filter(([, v]) => v?.author === 'ai').map(([k]) => k);
  check(ai.length === raws.length - 2 && !ai.includes(leftName), `Apply edits every ticked frame and not ${leftName} (${ai.length} of ${raws.length - 2})`);
  await p.screenshot({ path: `${out}shoot-look-applied.png` });
  await p.keyboard.press(process.platform === 'darwin' ? 'Meta+z' : 'Control+z');
  for (let i = 0; i < 50; i++) {
    r = await recipes();
    if (!Object.values(r).some((v) => v?.author === 'ai')) break;
    await p.waitForTimeout(200);
  }
  check(!Object.values(r).some((v) => v?.author === 'ai'), 'one Undo takes the look off every frame');
  await p.goto(`http://localhost:5203/?library=live#/cull`);

  // Crop like this: a hand crop on one frame is offered to the frame shot 39 s after it (the
  // a7R V samples DSC08006 and DSC08009), on its own or inside a fresh Shoot match.
  const byName = (n) => photos.find((ph) => ph.file_name === n)?.id;
  const [cropped, beside] = [byName('DSC08006.ARW'), byName('DSC08009.ARW')];
  if (cropped && beside) {
    const crop = { angle: 0, x: 0.1, y: 0.1, w: 0.7, h: 0.7 };
    const mine = (await api('read_recipe', { photo_id: cropped })).recipe ?? {};
    await api('edit', { photo_id: cropped, recipe: { ...mine, crop } });
    await p.reload();
    const line = p.getByTestId('crop-offers').or(p.getByTestId('pre-edits').filter({ hasText: 'with your crop' }));
    await line.first().waitFor({ timeout: 60000 });
    const text = await line.first().innerText();
    check(/crop/.test(text), `the crop is offered around it (${text.split('\n')[0]})`);
    await p.screenshot({ path: `${out}shoot-look-crop.png` });
    await p.getByTestId(/crop-offers-accept|pre-edits-accept/).first().click();
    let got;
    for (let i = 0; i < 50 && !got; i++) {
      got = (await api('read_recipe', { photo_id: beside })).recipe?.crop;
      if (!got) await p.waitForTimeout(200);
    }
    check(got && Math.abs(got.w - 0.7) < 1e-9 && Math.abs(got.x - 0.1) < 1e-9, `Accept gives the frame beside it the same crop (${JSON.stringify(got)})`);
  } else console.log('skip crop step: needs DSC08006 and DSC08009');
} catch (e) {
  failures.push(String(e).split('\n')[0]);
  await browser.contexts()[0]?.pages()[0]?.screenshot({ path: `${out}shoot-look-failed.png` }).catch(() => {});
} finally {
  await browser.close();
  await vite.close();
  serve.kill();
}
console.log(failures.length ? `\n${failures.length} failed:\n${failures.join('\n')}` : '\nall ok');
process.exit(failures.length ? 1 : 0);
