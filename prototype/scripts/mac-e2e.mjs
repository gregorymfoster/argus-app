// Mac check without screen control: the real engine (`argus serve`) behind the same React
// UI the app shows, driven headless in WebKit (the engine behind the app's WKWebView).
// Nothing asks for permission and nothing appears on screen, so it can run while Greg works.
//
// Part 1, Import from a memory card on an empty Library: a fake card (ARGUS_VOLUMES) holds
// RAW+JPEG pairs; the dialog finds it, shows every pair once, tagged RAW+JPEG, with RAW only
// ticked; Esc closes it; it imports RAWs only into a dated, space-free Shoot folder; ⌘, opens
// Settings. Part 2 runs scripts/live.mjs on the new Library (grid, flags, AI cull, Agent edit,
// Undo). Screenshots land in out/shots/mac-*.png and live-*.png.
//
//   node scripts/mac-e2e.mjs [raw-folder]   (default ../fixtures/raw-a7rv, else ../fixtures/raw)
//   ARGUS_BIN=../target/fast/argus          the engine binary (default shown)
//   PW_BROWSER=webkit|chromium              default webkit on macOS, chromium elsewhere

import { createServer } from 'vite';
import { chromium, webkit } from 'playwright';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { constants, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';

const root = new URL('../../', import.meta.url).pathname;
const rawDir = resolve(process.argv[2] ?? [join(root, 'fixtures/raw-a7rv'), join(root, 'fixtures/raw')].find((d) => existsSync(d) && raws(d).length) ?? '');
const bin = process.env.ARGUS_BIN ?? join(root, 'target/fast/argus');
const engine = process.env.PW_BROWSER ?? (process.platform === 'darwin' ? 'webkit' : 'chromium');
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

function raws(dir) {
  return readdirSync(dir).filter((f) => /\.(arw|nef|cr3|dng|raf)$/i.test(f)).sort();
}
const files = existsSync(rawDir) ? raws(rawDir).slice(0, 3) : [];
if (!files.length) {
  console.error(`no RAWs in ${rawDir || 'fixtures/raw-a7rv or fixtures/raw'}; pass a folder of RAWs`);
  process.exit(2);
}

// A fake memory card: the RAWs (copy-on-write clones on APFS, so instant and the originals are
// never touched; Import refuses symlinks that lead off the card) with a camera JPEG beside each.
const work = mkdtempSync(join(tmpdir(), 'argus-mac-e2e-'));
const volumes = join(work, 'volumes');
const dcim = join(volumes, 'Untitled', 'DCIM', '100MSDCF');
mkdirSync(dcim, { recursive: true });
const tinyJpeg = Buffer.from(
  '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAMCAgICAgMCAgIDAwMDBAYEBAQEBAgGBgUGCQgKCgkICQkKDA8MCgsOCwkJDRENDg8QEBEQCgwSExIQEw8QEBD/yQALCAABAAEBAREA/8wABgAQEAX/2gAIAQEAAD8A0s8g/9k=',
  'base64',
);
for (const f of files) {
  copyFileSync(join(rawDir, f), join(dcim, f), constants.COPYFILE_FICLONE);
  writeFileSync(join(dcim, f.replace(/\.[^.]+$/, '.JPG')), tinyJpeg);
}
const library = join(work, 'library');

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7878'], {
  stdio: ['ignore', 'inherit', 'inherit'],
  // No model in part 1: point Ollama at a closed port so Shoot naming falls back to the date.
  env: { ...process.env, ARGUS_VOLUMES: volumes, ARGUS_PROVIDER: 'ollama', OLLAMA_HOST: '127.0.0.1:9', ARGUS_COMPUTE: 'gentle' },
});
const api = (p, init) => fetch(`http://127.0.0.1:7878${p}`, init).then((r) => r.json());
for (let i = 0; ; i++) {
  try {
    await api('/api/list_shoots');
    break;
  } catch (e) {
    if (i > 100) throw e;
    await new Promise((r) => setTimeout(r, 100));
  }
}

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await (engine === 'webkit' ? webkit : chromium).launch(process.env.PW_CHROMIUM && engine === 'chromium' ? { executablePath: process.env.PW_CHROMIUM } : {});
const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));
console.log(`mac-e2e: ${files.length} RAW+JPEG pairs from ${rawDir}, ${engine}`);

try {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => failures.push(`page error: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && console.log(`     console: ${m.text()}`));
  const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

  await p.goto('http://localhost:5198/?library=live#/cull');
  check(!(await p.getByTestId('first-run').isVisible()), 'a Library given to the engine skips the first-launch screen');
  const dialog = p.getByTestId('import-dialog');
  await dialog.waitFor({ timeout: 15000 });
  check(true, 'an empty Library opens Import');
  await p.getByTestId('import-source').click();
  await p.getByTestId('import-card').first().click();
  await p.getByTestId('import-tile').first().waitFor({ timeout: 30000 });
  await p.waitForTimeout(1500); // card thumbnails
  await p.screenshot({ path: `${out}mac-import.png` });
  const tiles = await p.getByTestId('import-tile').count();
  const tags = await p.locator('.import-tag').allTextContents();
  check(tiles === files.length, `each RAW+JPEG pair shows once (${tiles} tiles for ${files.length} pairs)`);
  check(tags.length === files.length && tags.every((t) => t === 'RAW+JPEG'), `every tile is tagged RAW+JPEG (${tags.join(', ')})`);
  check((await p.getByTestId('import-raw-only').getAttribute('data-state')) === 'checked', 'RAW only is ticked by default');
  check((await p.getByTestId('import-source').innerText()).includes(`${files.length} photos`), 'the source names the photo count');

  await p.keyboard.press('Escape');
  await p.waitForTimeout(400);
  check(!(await dialog.isVisible()), 'Esc closes Import');
  await p.goto('http://localhost:5198/?library=live#/import');
  await dialog.waitFor();
  await p.getByTestId('import-source').click();
  await p.getByTestId('import-card').first().click();
  await p.getByTestId('import-tile').first().waitFor({ timeout: 30000 });
  await p.getByTestId('import-name').fill('Boat Trip');
  await p.getByTestId('import-start').click();
  await dialog.waitFor({ state: 'hidden', timeout: 120000 });
  // The Shoot opens while the Import runs; wait for its last Photo.
  await p.waitForTimeout(1500);
  await p.getByTestId('import-strip').waitFor({ state: 'detached', timeout: 600000 });
  const shoots = await api('/api/list_shoots');
  const photos = shoots.length ? await api(`/api/photos?shoot_id=${shoots[0].id}`) : [];
  check(shoots.length === 1 && photos.length === files.length, `import made one Shoot of ${files.length} Photos (${shoots.length} Shoots, ${photos.length} Photos)`);
  const stored = walk(library).map((f) => relative(library, f));
  const originals = stored.filter((f) => !f.startsWith('previews') && !/\.(db|db-wal|db-shm|json)$/.test(f) && !f.includes('backup'));
  check(originals.length > 0 && !originals.some((f) => /\.jpe?g$/i.test(f)), `RAW only kept the camera JPEGs on the card (${originals.join(', ')})`);
  check(!stored.some((f) => f.includes(' ')) && originals.some((f) => /\d{4}-\d{2}-\d{2}_boat-trip/i.test(f)), 'the Shoot folder is dated, with no spaces');
  await p.waitForTimeout(1500);
  await p.screenshot({ path: `${out}mac-imported.png` });

  await p.keyboard.press(`${mod}+Comma`);
  const settings = p.getByTestId('settings');
  await settings.waitFor({ timeout: 5000 }).catch(() => {});
  check(await settings.isVisible(), '⌘, opens Settings');
  await librarySettings(p, library, 'mac-settings');
  await p.keyboard.press('Escape');
  await p.waitForTimeout(400);
  check(!(await settings.isVisible()), 'Esc closes Settings');
  await ctx.close();
} catch (e) {
  failures.push(`crashed: ${e.message.split('\n')[0]}`);
  console.log(`FAIL ${e.message.split('\n')[0]}`);
  for (const pg of browser.contexts().flatMap((c) => c.pages())) {
    await pg.screenshot({ path: `${out}mac-fail.png` }).catch(() => {});
    console.log(`     screen at failure (out/shots/mac-fail.png): ${(await pg.locator('body').innerText().catch(() => '')).replace(/\s+/g, ' ').slice(0, 600)}`);
  }
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
  await once(bridge, 'exit');
}

// Part 3 (MAC_E2E_LIBRARY=<dir>): Greg's own Library opens straight to the grid, and Settings ›
// Library shows its path and sizes. Opens it as the app would, then only looks: no clicks that change anything.
async function librarySettings(p, dir, shot) {
  await p.getByTestId('settings-library').click();
  await p.waitForTimeout(500); // let the nav's background transition settle before reading or shooting it
  const path = p.getByTestId('library-path');
  await path.waitFor({ timeout: 10000 }).catch(() => {});
  const shown = (await path.getAttribute('title').catch(() => null)) ?? '';
  check(!!shown && realpathSync(shown) === realpathSync(dir), `Settings › Library shows the Library's path (${shown})`);
  const free = await p.getByTestId('free-space').innerText().catch(() => '');
  check(/\d.*(KB|MB|GB|TB)/.test(free), `Settings › Library shows sizes and free space (${free.trim()})`);
  const nav = await p.$$eval('[data-testid^=settings-]', (els) => els.filter((e) => e.tagName === 'BUTTON').map((e) => `${e.textContent}${e.getAttribute('aria-current') ? '*' : ''}:${getComputedStyle(e).backgroundColor}`));
  const filled = nav.filter((n) => !n.endsWith('rgba(0, 0, 0, 0)'));
  check(nav.filter((n) => n.includes('*')).length === 1 && filled.length === 1 && filled[0].startsWith('Library*'), `only Library is marked and filled in the Settings nav (${nav.join(', ')})`);
  await p.screenshot({ path: `${out}${shot}.png` });
}
if (!failures.length && process.env.MAC_E2E_LIBRARY) {
  const own = resolve(process.env.MAC_E2E_LIBRARY);
  const serve = spawn(bin, ['serve', '--library', own, '--port', '7878'], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, ARGUS_COMPUTE: 'gentle' } });
  for (let i = 0; ; i++) {
    try {
      await api('/api/list_shoots');
      break;
    } catch (e) {
      if (i > 100) throw e;
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  const vite = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
  await vite.listen();
  const b = await (engine === 'webkit' ? webkit : chromium).launch();
  try {
    const p = await (await b.newContext({ viewport: { width: 1440, height: 900 } })).newPage();
    await p.goto('http://localhost:5198/?library=live#/cull');
    await p.waitForSelector('.tile, [data-testid=import-dialog], [data-testid=first-run]', { timeout: 30000 });
    check(!(await p.getByTestId('first-run').isVisible()), `${own} opens without the first-launch screen`);
    await p.screenshot({ path: `${out}mac-own-library.png` });
    await p.keyboard.press(`${process.platform === 'darwin' ? 'Meta' : 'Control'}+Comma`);
    await p.getByTestId('settings').waitFor({ timeout: 5000 });
    await librarySettings(p, own, 'mac-own-library-settings');
  } catch (e) {
    failures.push(`own Library: ${e.message}`);
  } finally {
    await b.close();
    await vite.close();
    serve.kill();
    await once(serve, 'exit');
  }
}

// Part 2: the existing live smoke test (grid, flags, AI cull, Agent edit and Undo) on this Library.
if (!failures.length) {
  const live = spawnSync('node', [new URL('./live.mjs', import.meta.url).pathname, library], { stdio: 'inherit', env: { ...process.env, ARGUS_BIN: bin, PW_BROWSER: engine } });
  if (live.status !== 0) failures.push('live.mjs failed (see above)');
}
rmSync(work, { recursive: true, force: true });
if (failures.length) {
  console.error(`\n${failures.length} mac-e2e check(s) failed:\n${failures.join('\n')}`);
  process.exit(1);
}
console.log(`\nmac-e2e: all checks passed (screens in ${out})`);

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? walk(join(dir, d.name)) : [join(dir, d.name)]));
}
