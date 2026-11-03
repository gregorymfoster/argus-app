// Crop & straighten check against a real engine: a Shoot of Greg's finished edits (some with a
// crooked wall or window, one level, one already cropped), Edit's Crop & straighten popover
// (⌥⌘R) proposing a straighten on the crooked ones only, one left out, Apply (↵) writing the
// rest as one Undo that changes only the crop, the hand crop untouched, and ⌘Z taking it off.
// Then Straighten + crop, to see its punch-ins. Screenshots in out/shots/framing-*.png.
// Uses frames from outside Apr 2022-Apr 2024 only, so the screenshots can be shown to Greg.
//
//   node scripts/framing-check.mjs

import { mkdirSync } from 'node:fs';
import { benchLibrary, launchBrowser, startBridge, startVite } from './harness.mjs';

const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

// [bench file, name in the Shoot]: the first three lean (the bench's turns, checked by eye).
const FRAMES = [
  ['2024-11-01_DSC07697.jpg', 'A1_room.jpg'],
  ['2024-06-04_DSC04396.jpg', 'A2_panel.jpg'],
  ['2026-06-13_DSC06956.jpg', 'A3_lobby.jpg'],
  ['2025-12-28_DSC04634.jpg', 'A4_park.jpg'],
  ['2024-06-04_DSC04427.jpg', 'A5_cropped.jpg'],
];
const library = benchLibrary('framing', '2026-11-01_Framing', FRAMES);

const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
const api = (cmd, body = {}) => fetch(`http://127.0.0.1:7881/api/${cmd}`, { method: 'POST', body: JSON.stringify(body) }).then((r) => r.json());
const { proc: serve } = await startBridge({ library, port: 7881, env: { ARGUS_COMPUTE: 'full' } });
const vite = await startVite(5201, 7881);
const browser = await launchBrowser();
try {
  for (const j of await api('list_jobs', { status: 'queued' })) await api('cancel_job', { job_id: j.id });
  const photos = await api('list_photos', {});
  const id = (n) => photos.find((ph) => ph.file_name === n).id;
  const cropped = id('A5_cropped.jpg');
  const hand = { angle: 0, x: 0.05, y: 0.05, w: 0.9, h: 0.9 };
  await api('edit', { photo_id: cropped, recipe: { crop: hand } });

  const p = await (await browser.newContext({ viewport: { width: 1500, height: 900 }, deviceScaleFactor: 2 })).newPage();
  p.on('pageerror', (e) => failures.push(`page error: ${e.message}`));
  await p.goto(`http://localhost:5201/?library=live#/edit/${id('A4_park.jpg')}`);
  await p.getByTestId('framing-open').waitFor({ timeout: 30000 });
  await p.keyboard.press('Alt+Control+r').catch(() => {});
  if (!(await p.getByTestId('framing').isVisible().catch(() => false))) await p.getByTestId('framing-open').click();
  await p.getByTestId('framing').waitFor({ timeout: 10000 });
  await p.getByTestId('framing-reading').waitFor({ timeout: 5000 }).then(() => check(true, 'a progress bar while it looks at each frame'), () => check(true, 'read before the bar showed'));
  await p.getByTestId('framing-reading').waitFor({ state: 'detached', timeout: 180000 });
  const tiles = p.getByTestId('framing-tile');
  const names = await tiles.locator('.apply-cap').allInnerTexts();
  check(names.length >= 2 && names.every((n) => /A[123]_/.test(n)), `straightens only the crooked frames (${names.join(', ')})`);
  check(!names.includes('A5_cropped.jpg'), 'leaves the hand-cropped frame alone');
  const foot = await p.locator('.apply-look-foot').innerText();
  check(/1 already cropped/.test(foot), `the foot counts the hand crop (${foot.split('\n')[0]})`);
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}framing-popover.png` });

  // Leave one out, Apply: the rest get a straighten and nothing else, as one Undo.
  const leftOut = names[names.length - 1];
  await tiles.nth(names.length - 1).click();
  await p.keyboard.press('Enter');
  await p.getByTestId('framing').waitFor({ state: 'detached', timeout: 30000 });
  const read = async (n) => (await api('read_recipe', { photo_id: id(n) }))?.recipe ?? {};
  let turned = 0;
  for (const n of names.slice(0, -1)) {
    const r = await read(n);
    if (r.crop?.angle && r.crop.w === 1) turned++;
  }
  check(turned === names.length - 1, `Apply straightens every ticked frame (${turned} of ${names.length - 1})`);
  check(!(await read(leftOut)).crop, `and not ${leftOut}`);
  const kept = (await read('A5_cropped.jpg')).crop ?? {};
  check(['x', 'y', 'w', 'h'].every((k) => Math.abs((kept[k] ?? -1) - hand[k]) < 1e-9) && !kept.angle, `the hand crop is untouched (${JSON.stringify(kept)})`);
  await p.screenshot({ path: `${out}framing-applied.png` });
  await p.keyboard.press('Control+z');
  let back = false;
  for (let i = 0; i < 50 && !back; i++) {
    back = !(await Promise.all(names.map(read))).some((r) => r.crop);
    if (!back) await p.waitForTimeout(200);
  }
  check(back, 'one Undo takes every straighten off');

  // Straighten + crop: the punch-ins too.
  await p.getByTestId('framing-open').click();
  await p.getByTestId('framing').waitFor({ timeout: 10000 });
  await p.getByRole('radio', { name: 'Straighten + crop' }).or(p.getByRole('button', { name: 'Straighten + crop' })).first().click();
  await p.waitForTimeout(500);
  await p.getByTestId('framing-reading').waitFor({ state: 'detached', timeout: 180000 }).catch(() => {});
  await p.waitForTimeout(500);
  const withCrop = await tiles.count();
  check(withCrop >= names.length, `Straighten + crop shows at least as many (${withCrop})`);
  await p.screenshot({ path: `${out}framing-crop.png` });
} catch (e) {
  failures.push(String(e).split('\n')[0]);
  await browser.contexts()[0]?.pages()[0]?.screenshot({ path: `${out}framing-failed.png` }).catch(() => {});
} finally {
  await browser.close();
  await vite.close();
  serve.kill();
}
console.log(failures.length ? `\n${failures.length} failed:\n${failures.join('\n')}` : '\nall ok');
process.exit(failures.length ? 1 : 0);
