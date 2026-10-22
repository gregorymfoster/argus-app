// End-to-end self-test of the real desktop app: the Tauri binary, its IPC and the
// argus:// preview protocol, driven over WebDriver (tauri-driver + WebKitWebDriver).
// Linux only, because tauri-driver has no macOS support; it runs in the cloud container
// under Xvfb. On the Mac, open the built .app by hand instead.
//
//   npm run e2e                 (from app/; builds first with `npm run build:debug`)
//   ARGUS_APP_BIN=path          to test another binary
//   E2E_QUICK=1                 stop after Import, the grid and one flag (`npm run e2e:quick`)
//
// It starts on an empty temporary Library (Import opens), imports a folder of generated
// JPEGs through the app's own `call` command, then checks the grid shows engine
// previews, P writes a Pick to the Library, the loupe opens, flagging jumps to the next
// Photo, Compare opens and closes, the provider menu names the real Provider, the Agent's
// AI cull lands (against a mock Ollama) and its conversation survives the reload, Edit
// sliders re-render through the engine and save the person's Recipe, the Agent edits the
// open Photo on top of that (mock model) and Undo in the chat puts it back, Export Picks
// writes dated JPEGs, the daily Backup exists, and flags and the conversation survive a
// relaunch. Screenshots go to out/shots/app-*.png at the repo root.

import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const repo = new URL('../../', import.meta.url).pathname;
const bin = process.env.ARGUS_APP_BIN ?? join(repo, 'app/target/debug/argus-app');
const out = join(repo, 'out/shots');
mkdirSync(out, { recursive: true });

const tmp = mkdtempSync(join(tmpdir(), 'argus-e2e-'));
const library = join(tmp, 'library');
// A memory card the app should find by itself: a "mounted volume" with a DCIM folder.
const volumes = join(tmp, 'Volumes');
const folder = join(volumes, 'SONY_A7/DCIM/100MSDCF');
mkdirSync(folder, { recursive: true });
mkdirSync(join(volumes, 'Backup-Drive'));
// Eight small distinct JPEGs (Pillow, which the bench already needs); the last two get
// imported first, so the card dialog has to leave them out.
execFileSync('python3', ['-I', '-c', `
import sys
from PIL import Image
for i in range(8):
    im = Image.new('RGB', (900, 600), (40 + i * 30, 90, 200 - i * 25))
    im.save(f"{sys.argv[1]}/IMG_{i:04d}.jpg", quality=90)
`, folder]);

// A stand-in for Ollama on :7879 for the Agent's AI cull: answers the model load, then
// picks photo 1, stacks 1 and 2, rejects the rest.
let modelCalls = 0;
let agentCalls = 0;
let namingCalls = 0;
const mock = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    res.setHeader('content-type', 'application/json');
    // Settings › AI and Compute lists the installed models and which one is loaded.
    if (req.url === '/api/tags') return res.end(JSON.stringify({ models: [{ name: 'qwen3.8:27b', size: 17e9 }] }));
    if (req.url === '/api/ps') return res.end(JSON.stringify({ models: [] }));
    const r = JSON.parse(body || '{}');
    if (!r.messages?.length) return res.end(JSON.stringify({ message: { role: 'assistant', content: '' }, done: true }));
    // An Agent turn (step schema): edit the Photo in focus, then say what changed.
    if (r.format?.required?.includes('calls')) {
      agentCalls++;
      const focus = Number(/In focus: Photo (\d+)/.exec(r.messages[1].content)?.[1]);
      const done = r.messages[r.messages.length - 1].content.includes('edit_recipe ->');
      const step = done
        ? { say: 'Mock edit: Temperature +12, Contrast -15.', calls: [] }
        : { say: 'Warming', calls: [{ tool: 'edit_recipe', args: { photo_ids: [focus], set: { temp: 12, contrast: -15 }, relative: true } }] };
      return res.end(JSON.stringify({ message: { content: JSON.stringify(step) }, done: true }));
    }
    if (r.format?.properties?.name) {
      namingCalls++;
      return res.end(JSON.stringify({ message: { content: JSON.stringify({ name: 'Pickleball' }) }, done: true }));
    }
    modelCalls++;
    const n = r.messages.filter((m) => m.images).length;
    const photos = Array.from({ length: n }, (_, i) => ({ n: i + 1, verdict: i === 0 ? 'pick' : 'reject', reason: i === 0 ? 'best of 2' : 'weaker frame', stack: i < 2 ? 1 : null, rank: i < 2 ? i + 1 : null }));
    res.end(JSON.stringify({ message: { content: JSON.stringify({ summary: 'Mock cull.', photos }) }, done: true }));
  });
}).listen(7879, '127.0.0.1');

const port = 4444;
// A virtual screen when there is no real one (the cloud container).
let display = process.env.DISPLAY;
const xvfb = display ? null : spawn('Xvfb', [(display = `:${90 + Math.floor(Math.random() * 9)}`), '-screen', '0', '1440x900x24'], { stdio: 'ignore' });
const driver = spawn('tauri-driver', [], { stdio: ['ignore', 'ignore', 'inherit'], env: { ...process.env, DISPLAY: display, ARGUS_LIBRARY: library, ARGUS_CONFIG: join(tmp, 'config/libraries.json'), ARGUS_VOLUMES: volumes, OLLAMA_HOST: '127.0.0.1:7879', ARGUS_PROVIDER: 'ollama', ARGUS_CULL_CHECK: 'model', ARGUS_COMPUTE: 'full' } });

const wd = async (method, path, body) => {
  const r = await fetch(`http://127.0.0.1:${port}${path}`, { method, headers: { 'Content-Type': 'application/json' }, body: body && JSON.stringify(body) });
  const j = await r.json();
  if (j.value?.error) throw new Error(`${method} ${path}: ${j.value.error}: ${j.value.message}`);
  return j.value;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; ; i++) {
  try {
    await fetch(`http://127.0.0.1:${port}/status`);
    break;
  } catch (e) {
    if (i > 100) throw e;
    await sleep(100);
  }
}

const failures = [];
// Thrown to end an E2E_QUICK run early; not a failure.
const QUICK_DONE = Symbol('quick run done');
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
let sid;
const js = (script, args = []) => wd('POST', `/session/${sid}/execute/sync`, { script, args });
const jsAsync = (script, args = []) => wd('POST', `/session/${sid}/execute/async`, { script, args });
const find = (css) => wd('POST', `/session/${sid}/element`, { using: 'css selector', value: css });
const waitFor = async (css, ms = 15000) => {
  for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(150)) {
    if (await js(`return !!document.querySelector(arguments[0])`, [css])) return;
  }
  throw new Error(`timed out waiting for ${css}`);
};
const shot = async (name) => writeFileSync(join(out, `app-${name}.png`), Buffer.from(await wd('GET', `/session/${sid}/screenshot`), 'base64'));
const invoke = (command, args) =>
  jsAsync(`const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke('call', { command: arguments[0], args: arguments[1] }).then((v) => done({ v }), (e) => done({ e: String(e) }))`, [command, args]).then((r) => {
    if (r.e) throw new Error(r.e);
    return r.v;
  });
const open = async () => ({ sessionId: sid } = await wd('POST', '/session', { capabilities: { alwaysMatch: { 'tauri:options': { application: bin } } } }));
const press = async (key) => {
  await wd('POST', `/session/${sid}/actions`, { actions: [{ type: 'key', id: 'kb', actions: [{ type: 'keyDown', value: key }, { type: 'keyUp', value: key }] }] });
};

try {
  await open();

  await waitFor('[data-testid="import-dialog"]');
  check(true, 'an empty Library opens on Import');
  await js(`document.querySelector('.fixed.inset-0').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })); document.querySelector('.fixed.inset-0').click()`);
  await sleep(300);
  const stays = await js(`return [!!document.querySelector('[data-testid="import-dialog"]'), !!document.querySelector('[data-testid="import-cancel"]'), !!document.querySelector('[data-testid="import-dialog"] [aria-label="Close"]')]`);
  check(stays.every(Boolean), 'a click outside does not close Import, and it has Cancel and ✕');
  await press('\uE00C');
  for (const t0 = Date.now(); Date.now() - t0 < 3000 && (await js(`return !!document.querySelector('[data-testid="import-dialog"]')`)); ) await sleep(100);
  const afterEsc = await js(`return [location.hash, !!document.querySelector('[data-testid="import-dialog"]'), document.activeElement?.outerHTML.slice(0, 80)]`);
  check(!afterEsc[1], `Esc closes Import, even on an empty Library (now at ${afterEsc[0]})`);

  await wd('POST', `/session/${sid}/actions`, { actions: [{ type: 'key', id: 'kb', actions: [{ type: 'keyDown', value: '\uE009' }, { type: 'keyDown', value: ',' }, { type: 'keyUp', value: ',' }, { type: 'keyUp', value: '\uE009' }] }] });
  await waitFor('[data-testid="settings"]', 5000).catch(() => {});
  check(await js(`return !!document.querySelector('[data-testid="settings"] [data-testid="settings-ai"]')`), 'Ctrl/⌘ + , opens Settings');
  await shot('settings');
  await press('\uE00C');
  await sleep(300);
  check(!(await js(`return !!document.querySelector('[data-testid="settings"]')`)), 'Esc closes Settings');
  // Updates: the app answers update_status; a test build has no update feed, so it says so
  // and offers no Restart to update.
  const upd = await jsAsync(`const done = arguments[arguments.length - 1]; window.__TAURI_INTERNALS__.invoke('update_status').then((v) => done({ v }), (e) => done({ e: String(e) }))`);
  check(upd.v?.state === 'off' && /^\d+\.\d+\.\d+/.test(upd.v?.current ?? ''), `the app reports its version and that this build doesn't update itself (${JSON.stringify(upd)})`);
  await js(`window.dispatchEvent(new CustomEvent('argus:settings', { detail: 'updates' }))`);
  await waitFor('[data-testid="settings-updates-page"]', 5000).catch(() => {});
  const updPage = await js(`return [document.querySelector('[data-testid="settings-updates-page"]')?.textContent ?? '', !!document.querySelector('[data-testid="update-restart"]')]`);
  check(updPage[0].includes(`Argus ${upd.v?.current}`) && !updPage[1], `Settings › Updates shows the version, no Restart button (${updPage[0].slice(0, 80)})`);
  await press('\uE00C');
  await sleep(300);
  await js(`location.hash = '#/import'`);
  await waitFor('[data-testid="import-dialog"]');
  await shot('empty');

  // Import finds the card by itself, shows every photo on it and names the Shoot.
  await waitFor('[data-testid="import-tile"]');
  // Radix menus open on pointerdown, which a script's click() doesn't send.
  const openMenu = (id) => js(`document.querySelector('[data-testid="${id}"]').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))`);
  await openMenu('import-source');
  await waitFor('[data-testid="import-card"]');
  check((await js(`return document.querySelectorAll('[data-testid="import-card"]').length`)) === 1, 'Import finds the memory card (and not the drive without DCIM)');
  await js(`document.querySelector('[data-testid="import-card"]').click()`);
  await waitFor('[data-testid="import-tile"]');
  check((await js(`return document.querySelectorAll('[data-testid="import-tile"][aria-pressed="true"]').length`)) === 8, 'every photo on a fresh card is selected');

  // Two of them were imported before: the dialog must leave those out.
  const earlier = await invoke('import_folder', { folder, shoot_name: 'Earlier', files: [6, 7].map((i) => join(folder, `IMG_${String(i).padStart(4, '0')}.jpg`)) });
  check(earlier.imported === 2, `importing chosen files imports only those (${earlier.imported})`);
  await js(`location.hash = '#/import'; location.reload()`);
  // New photos (the default) hides the two; All photos shows them, unticked.
  for (const t0 = Date.now(); Date.now() - t0 < 15000 && (await js(`return document.querySelectorAll('[data-testid="import-tile"]').length`)) !== 6; ) await sleep(150);
  check((await js(`return document.querySelectorAll('[data-testid="import-tile"]').length`)) === 6, 'New photos shows only the 6 not yet imported');
  await openMenu('import-show');
  await waitFor('[data-testid="import-show-all"]');
  await js(`document.querySelector('[data-testid="import-show-all"]').click()`);
  await waitFor('[data-testid="import-tile"][data-imported]');
  const states = await js(`return [...document.querySelectorAll('[data-testid="import-tile"]')].map((t) => [t.title, t.getAttribute('aria-pressed'), !!t.dataset.imported])`);
  const off = states.filter(([, on]) => on === 'false').map(([n]) => n);
  check(states.length === 8 && off.join() === 'IMG_0006.jpg,IMG_0007.jpg' && states.filter(([, , imp]) => imp).length === 2,
    `already-imported photos are marked and unselected (${off.join() || 'none'})`);
  for (const t0 = Date.now(); Date.now() - t0 < 15000 && !/_Pickleball$/.test(await js(`return document.querySelector('[data-testid="import-name"]').value`)); ) await sleep(150);
  const suggested = await js(`return document.querySelector('[data-testid="import-name"]').value`);
  check(/^\d{4}-\d{2}-\d{2}_Pickleball$/.test(suggested) && namingCalls >= 1, `the local model suggested a dated Shoot name ("${suggested}")`);
  for (const t0 = Date.now(); Date.now() - t0 < 15000 && (await js(`return document.querySelectorAll('[data-testid="import-tile"] img').length`)) < 8; ) await sleep(150);
  const thumbs = await js(`return [...document.querySelectorAll('[data-testid="import-tile"] img')].filter((i) => i.naturalWidth > 0).length`);
  check(thumbs === 8, `the card's photos show as thumbnails (${thumbs})`);
  // A real card holds hundreds: tiles must keep their size and the grid scroll (WebKit once squashed them).
  const layout = await js(`const g = document.querySelector('[data-testid="import-grid"]'); const t = g.firstElementChild; for (let i = 0; i < 200; i++) g.appendChild(t.cloneNode(true));
    const h = [...g.children].map((c) => c.getBoundingClientRect().height); let box = g.parentElement; while (box && !/auto|scroll/.test(getComputedStyle(box).overflowY)) box = box.parentElement;
    const r = [Math.min(...h), box.scrollHeight > box.clientHeight, box.clientHeight]; for (let i = 0; i < 200; i++) g.lastElementChild.remove(); return r`);
  check(layout[0] > 60 && layout[1] && layout[2] > 200, `a full card keeps tiles full size and scrolls (smallest tile ${Math.round(layout[0])}px, box ${layout[2]}px)`);
  await openMenu('import-show');
  await waitFor('[data-testid="import-show-new"]');
  await js(`document.querySelector('[data-testid="import-show-new"]').click()`);
  await sleep(200);
  check(/Import 6\b/.test(await js(`return document.querySelector('[data-testid="import-start"]').textContent`)), 'the Import button counts the 6 new photos');
  const space = await js(`return document.querySelector('[data-testid="import-space"]')?.textContent ?? ''`);
  check(/6 photos · .+ · (about|a few).+\s.+ free/.test(space), `Import shows the photos, space and time it needs and what's free ("${space}")`);
  // The background cull after import is covered by its own step below, so it runs by hand here.
  await js(`const c = document.querySelector('[data-testid="import-auto-cull"]'); if (c.getAttribute('data-state') === 'checked') c.click()`);
  // So are Suggestions after import (the Queue step below), so they stay off here too.
  await js(`const c = document.querySelector('[data-testid="import-suggest"]'); if (c.getAttribute('data-state') === 'checked') c.click()`);
  await shot('import-card');
  // Picking the card again starts a fresh scan: the old suggestion must not linger meanwhile.
  await openMenu('import-source');
  await waitFor('[data-testid="import-card"]');
  await js(`document.querySelector('[data-testid="import-card"]').click()`);
  await sleep(50);
  const mid = await js(`return document.querySelector('[data-testid="import-name"]').value`);
  check(!/Pickleball/.test(mid), `changing source clears the old suggested name (field: "${mid}")`);
  for (const t0 = Date.now(); Date.now() - t0 < 15000 && !/_Pickleball$/.test(await js(`return document.querySelector('[data-testid="import-name"]').value`)); ) await sleep(150);
  // Whatever Greg types is saved with the date first and no spaces.
  const typeInto = (text) =>
    js(`const i = document.querySelector('[data-testid="import-name"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(i, arguments[0]); i.dispatchEvent(new Event('input', { bubbles: true }))`, [text]);
  await typeInto('Boat Trip');
  for (const t0 = Date.now(); Date.now() - t0 < 5000 && !/Saved as/.test(await js(`return document.querySelector('[data-testid="import-name-hint"]')?.textContent ?? ''`)); ) await sleep(100);
  const savedAs = await js(`return document.querySelector('[data-testid="import-name-hint"]')?.textContent ?? ''`);
  check(/Saved as \d{4}-\d{2}-\d{2}_Boat-Trip/.test(savedAs), `a typed name shows how it will be saved ("${savedAs}")`);
  await typeInto(suggested);
  await js(`document.querySelector('[data-testid="import-start"]').click()`);
  // The grid behind still shows the earlier Shoot until the import opens the new one.
  for (const t0 = Date.now(); Date.now() - t0 < 20000 && (await js(`return !!document.querySelector('[data-testid="import-dialog"]') || document.querySelectorAll('.tile img').length !== 6`).catch(() => true)); ) await sleep(200);
  await sleep(1000);
  const imgs = await js(`return [...document.querySelectorAll('.tile img')].map((i) => [i.getAttribute('src'), i.naturalWidth])`);
  check(imgs.length === 6, `the grid shows the new Shoot's 6 Photos (${imgs.length} tiles)`);
  const shoots = await invoke('list_shoots', {});
  check(shoots.some((s) => /^\d{4}-\d{2}-\d{2}_Pickleball$/.test(s.name) && s.photos === 6), `the new Shoot is named <date>_Pickleball (${shoots.map((s) => `${s.name}:${s.photos}`).join(', ')})`);
  check(imgs.every(([s, w]) => s.startsWith('argus://') && w > 0), 'tiles load engine previews over argus://');
  await shot('cull');

  const photos = await invoke('photos', {});
  const first = photos[0].id;
  await (async () => {
    const el = await find(`[data-testid="tile-${first}"]`);
    await wd('POST', `/session/${sid}/element/${Object.values(el)[0]}/click`, {});
  })();
  await press('p');
  await sleep(600);
  const after = (await invoke('photos', {})).find((p) => p.id === first);
  check(after.verdict === 'pick' && after.verdict_by === 'person', `P wrote a Pick to the Library (got ${after.verdict} by ${after.verdict_by})`);
  if (process.env.E2E_QUICK) throw QUICK_DONE;

  await js(`location.hash = '#/loupe/${first}'`);
  await waitFor('.loupe img, [data-testid="loupe"] img, img');
  await sleep(800);
  const loupe = await js(`return [...document.querySelectorAll('img')].filter((i) => i.naturalWidth > 0 && i.getBoundingClientRect().width > 400).length`);
  check(loupe > 0, 'the loupe shows the Photo large');
  const side0 = await js(`return document.querySelector('aside.side')?.textContent ?? ''`);
  check(!/Claude/.test(side0) && !/suggests/.test(side0), `before any cull the loupe claims no AI suggestion ("${side0.slice(0, 40)}")`);
  await shot('loupe');

  // Flagging jumps to the next Photo: X right after P lands on the second Photo.
  await js(`location.hash = '#/cull'`);
  await waitFor('.tile img');
  await (async () => {
    const el = await find(`[data-testid="tile-${first}"]`);
    await wd('POST', `/session/${sid}/element/${Object.values(el)[0]}/click`, {});
  })();
  await press('p');
  await press('x');
  await sleep(600);
  const second = (await invoke('photos', {})).find((p) => p.id === photos[1].id);
  check(second.verdict === 'reject', `P then X flags the next Photo (second got ${second.verdict})`);

  await js(`location.hash = '#/compare/${photos[0].id},${photos[1].id}'`);
  await sleep(1200);
  const side = await js(`return [...document.querySelectorAll('img')].filter((i) => i.naturalWidth > 0 && i.getBoundingClientRect().width > 200).length`);
  check(side >= 2, `Compare shows the Photos side by side (${side} large)`);
  const compareText = await js(`return document.body.textContent`);
  check(!/ranks .* best/.test(compareText) && !/focus 0\.00/.test(compareText), 'before any cull Compare shows no AI ranking and no empty focus score');
  await shot('compare');
  await press('\uE00C'); // Escape
  await sleep(400);
  check((await js(`return location.hash`)).startsWith('#/cull'), 'Escape leaves Compare for the grid');

  // The provider menu names the Provider a cull really uses (ARGUS_PROVIDER=ollama here).
  const providerLabel = await js(`return document.querySelector('[data-testid="provider"]')?.textContent ?? ''`);
  check(/Ollama/.test(providerLabel) && !/^Claude/.test(providerLabel), `the Agent's provider menu shows the real Provider ("${providerLabel.trim()}")`);

  // The Agent's "cull this shoot" runs the real engine cull against the mock model.
  const ask = await find('#ask-cull');
  await wd('POST', `/session/${sid}/element/${Object.values(ask)[0]}/value`, { text: 'cull this shoot\uE007' });
  let culled = [];
  for (const t0 = Date.now(); Date.now() - t0 < 30000; await sleep(300)) {
    culled = await invoke('photos', {});
    if (culled.some((p) => p.verdict_by === 'ai')) break;
  }
  check(modelCalls === 1, `the Agent sent one Shoot-wide request to the model (${modelCalls})`);
  const ai = culled.filter((p) => p.verdict_by === 'ai');
  check(ai.length === 4, `AI Verdicts landed on the 4 Photos the person hadn't flagged (${ai.length})`);
  check(culled.find((p) => p.id === first).verdict_by === 'person', 'the person\'s own flags were kept');
  await sleep(3500); // the UI reloads from the Library 2.5 s after a cull
  await waitFor('.tile img');
  // The model stacked two Photos (and Argus joins look-alike frames, which these flat test
  // images are), so every Stack shows one tile until expanded.
  const tiles = await js(`return [...document.querySelectorAll('[data-testid^=tile-]')].map((t) => t.dataset.verdict)`);
  const inShoot = culled.filter((p) => p.shoot_id === culled.find((x) => x.id === first).shoot_id);
  const stacked = new Set(inShoot.filter((p) => p.stack != null).map((p) => p.stack));
  const collapsedTiles = inShoot.filter((p) => p.stack == null).length + stacked.size;
  check(stacked.size > 0 && tiles.length === collapsedTiles && collapsedTiles < inShoot.length && tiles.includes('pick'),
    `the grid shows the AI's Verdicts with the Stack collapsed (${tiles.join(' ')}; Library: ${inShoot.map((p) => `${p.verdict}/${p.stack}`).join(' ')})`);
  await shot('ai-cull');
  const chat = await js(`return document.querySelector('.agent-body')?.textContent ?? ''`);
  check(chat.includes('cull this shoot') && chat.includes('Mock cull.'), 'the Agent conversation survives the reload after a cull');

  // Edit: sliders drive the engine's render_photo, and the change is saved as the person's Recipe.
  await js(`location.hash = '#/edit/${first}'`);
  const renderedSrc = () => js(`return [...document.querySelectorAll('.stage img')].map((i) => i.getAttribute('src')).find((s) => s?.startsWith('data:image/jpeg')) ?? ''`);
  let before = '';
  for (const t0 = Date.now(); !before && Date.now() - t0 < 20000; await sleep(200)) before = await renderedSrc();
  check(before.length > 1000, 'the Edit screen shows the engine\'s render of the Photo');
  const thumb = await js(`const s = [...document.querySelectorAll('.slider')].find((x) => x.querySelector('.slider-label')?.textContent === 'Exposure'); return s?.querySelector('[role=slider]') ?? null`);
  await wd('POST', `/session/${sid}/element/${Object.values(thumb)[0]}/click`, {});
  for (let i = 0; i < 4; i++) await press('\uE014'); // ArrowRight, +0.05 EV each
  let after2 = before;
  for (const t0 = Date.now(); after2 === before && Date.now() - t0 < 20000; await sleep(200)) after2 = await renderedSrc();
  check(after2 !== before, 'moving Exposure re-renders the Photo through the engine');
  await sleep(1200); // edits save once the slider settles
  const saved = await invoke('read_recipe', { photo_id: first });
  check(saved?.author === 'person' && saved.recipe.global.exposure > 0, `the edit was saved as the person's Recipe (exposure ${saved?.recipe?.global?.exposure})`);
  await shot('edit');

  // The Agent edits the open Photo on top of the person's own edit, as one undoable batch.
  const askEdit = await find(`#ask-edit\\:${first}`);
  await wd('POST', `/session/${sid}/element/${Object.values(askEdit)[0]}/value`, { text: 'warmer, less contrast\uE007' });
  let agentEdit = null;
  for (const t0 = Date.now(); Date.now() - t0 < 30000; await sleep(300)) {
    agentEdit = await invoke('read_recipe', { photo_id: first });
    if (agentEdit?.author === 'ai') break;
  }
  const g = agentEdit?.recipe?.global ?? {};
  check(agentEdit?.author === 'ai' && g.temp === 12 && g.contrast === -15 && g.exposure === saved.recipe.global.exposure, `the Agent's edit landed on top of the person's (${JSON.stringify(g)}, ${agentCalls} model turns)`);
  await waitFor('.agent .prose');
  await shot('agent-edit');
  await js(`[...document.querySelectorAll('.agent button')].find((b) => b.textContent.trim() === 'Undo')?.click()`);
  let back = agentEdit;
  for (const t0 = Date.now(); back?.recipe?.global?.temp && Date.now() - t0 < 10000; await sleep(300)) back = await invoke('read_recipe', { photo_id: first });
  check(!back?.recipe?.global?.temp && back?.recipe?.global?.exposure === saved.recipe.global.exposure, `Undo in the chat put back the person's edit (${JSON.stringify(back?.recipe?.global)})`);
  await js(`location.hash = '#/cull'`);

  // Card imports copied the originals into the Library; Settings › Library shows where it is and its disk use.
  const disk = await invoke('library_storage', {});
  check(disk.originals_in_library.files === 8 && disk.originals_elsewhere.files === 0 && disk.missing === 0,
    `card imports copied every original into the Library (${disk.originals_in_library.files} in, ${disk.originals_elsewhere.files} elsewhere)`);
  await js(`window.dispatchEvent(new CustomEvent('argus:settings', { detail: 'library' }))`);
  await waitFor('[data-testid="library-path"]');
  const shown = await js(`return [document.querySelector('[data-testid="library-path"]').title, document.querySelector('[data-testid="free-space"]').textContent]`);
  check(shown[0] === disk.library && /\d/.test(shown[1]), `Settings › Library shows the Library folder and free space (${shown.join(', ')})`);
  await shot('settings-library');
  await press('\uE00C');
  await sleep(300);

  // Export Picks: the dialog renders each Pick through the engine into a Shoot folder.
  // (The folder picker is native, so the test sets the remembered location instead.)
  const exportDir = join(tmp, 'exports');
  await js(`localStorage.setItem('argus-export', JSON.stringify({ folder: arguments[0] }))`, [exportDir]);
  await js(`location.hash = '#/cull'`);
  await waitFor('.tile img');
  // Export lives in menus and ⌘E / ⌘⇧E (Design pass v2), not the header.
  await js(`document.dispatchEvent(new KeyboardEvent('keydown', { key: 'e', code: 'KeyE', ctrlKey: true, bubbles: true }))`);
  await waitFor('[data-testid="export-start"]');
  await js(`document.querySelector('[data-testid="export-start"]').click()`);
  for (const t0 = Date.now(); Date.now() - t0 < 30000; await sleep(200)) {
    if (/^Done\./.test(await js(`return document.querySelector('[data-testid="export-status"]')?.textContent ?? ''`))) break;
  }
  await sleep(300);
  await shot('exported');
  const picks = (await invoke('photos', {})).filter((p) => p.verdict === 'pick').length;
  const shootDirs = readdirSync(exportDir);
  const exported = shootDirs.length === 1 ? readdirSync(join(exportDir, shootDirs[0])) : [];
  check(picks > 0 && exported.length === picks && exported.every((f) => f.endsWith('.jpg') && !f.includes(' ')),
    `Export Picks wrote ${picks} JPEG(s) into a Shoot folder (${shootDirs.join(', ')}: ${exported.join(', ')})`);
  await press('\uE00C');
  await sleep(300);

  // Flags survive a relaunch.
  await wd('DELETE', `/session/${sid}`);
  await open();
  await waitFor(`[data-testid="tile-${first}"]`);
  const v = await js(`return document.querySelector('[data-testid="tile-${first}"]').dataset.verdict`);
  check(v === 'pick', `after a relaunch the grid still shows the Pick (${v || 'none'})`);
  const chatAgain = await js(`return document.querySelector('.agent-body')?.textContent ?? ''`);
  check(chatAgain.includes('Mock cull.'), 'after a relaunch the Agent conversation is still there');
  // The first launch had no Library yet; this one backs it up (ADR 0009).
  let backups = [];
  for (const t0 = Date.now(); Date.now() - t0 < 10000 && !backups.length; await sleep(200)) {
    backups = existsSync(join(library, 'backups')) ? readdirSync(join(library, 'backups')).filter((f) => f.endsWith('.db')) : [];
  }
  check(backups.length >= 1, `on launch the app made its daily Library Backup (${backups.join(', ')})`);
} catch (e) {
  if (e !== QUICK_DONE) {
    failures.push(String(e));
    console.log(`FAIL ${e}`);
  }
} finally {
  if (sid) await wd('DELETE', `/session/${sid}`).catch(() => {});
  driver.kill();
  xvfb?.kill();
  mock.close();
}
if (failures.length) {
  console.error(`\n${failures.length} app check(s) failed`);
  process.exit(1);
}
console.log(process.env.E2E_QUICK ? '\napp e2e (quick): all checks passed' : '\napp e2e: all checks passed');
