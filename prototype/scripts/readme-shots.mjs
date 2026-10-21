// Screenshots and short screen recordings of the real app for the public README (Greg,
// 2026-10-21). Like release-shots.mjs it works on a copy of a Library that must hold only the
// public-domain sample card (docs/release.md), never Greg's photos or the bench.
//
//   node scripts/readme-shots.mjs <culled-library> <card-folder> <edit-id> <agent-id>
//
// <culled-library> has the card imported, AI-culled and with Suggestions for <edit-id>;
// <card-folder> is the card itself, imported again from a fake memory card for the Import clip.
// The Agent request runs for real, so set ARGUS_PROVIDER (claude-code in the cloud).
// PNG stills and WebM clips land in out/readme-shots/; scripts/readme-media.sh shrinks them.
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { copyFileSync, cpSync, mkdirSync, mkdtempSync, readdirSync, renameSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [source, card, editId, agentId] = process.argv.slice(2);
if (!agentId) {
  console.error('usage: node scripts/readme-shots.mjs <culled-library> <card-folder> <edit-id> <agent-id>');
  process.exit(2);
}
const only = (process.env.ONLY ?? '').split(',').filter(Boolean);
const want = (name) => !only.length || only.includes(name);
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/readme-shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const tmp = mkdtempSync(join(tmpdir(), 'argus-readme-'));
const library = join(tmp, 'lib');
cpSync(source, library, { recursive: true });

// A fake memory card for the Import clip, and a stand-in model that names the Shoot.
const volumes = join(tmp, 'Volumes');
const dcim = join(volumes, 'Untitled', 'DCIM', '100MSDCF');
mkdirSync(dcim, { recursive: true });
for (const f of readdirSync(card).filter((f) => /\.jpe?g$/i.test(f))) copyFileSync(join(card, f), join(dcim, f));
const mock = httpServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    res.setHeader('content-type', 'application/json');
    if (!body || !JSON.parse(body).messages?.length) return res.end(JSON.stringify({ models: [], message: { content: '' }, done: true }));
    setTimeout(() => res.end(JSON.stringify({ message: { content: '{"name":"Coast Trip"}' }, done: true })), 900);
  });
});
await new Promise((r) => mock.listen(0, r));

const serve = (lib, port, env = {}) => {
  const proc = spawn(bin, ['serve', '--library', lib, '--port', String(port)], { stdio: ['ignore', 'ignore', 'inherit'], env: { ...process.env, ARGUS_QUEUE: 'off', ...env } });
  return proc;
};
const ready = async (port) => {
  for (let i = 0; i < 100; i++) {
    if (await fetch(`http://127.0.0.1:${port}/api/list_shoots`).then((r) => r.ok, () => false)) return;
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`bridge on ${port} never came up`);
};
const main = serve(library, 7871);
const fresh = serve(join(tmp, 'fresh'), 7872, {
  ARGUS_VOLUMES: volumes,
  OLLAMA_HOST: `127.0.0.1:${mock.address().port}`,
  ARGUS_PROVIDER: 'ollama',
  ARGUS_MODEL_LOCK_DIR: mkdtempSync(join(tmpdir(), 'argus-model-')),
  ARGUS_FOREGROUND_DIR: mkdtempSync(join(tmpdir(), 'argus-fg-')),
});
await Promise.all([ready(7871), ready(7872)]);

const vite = async (port, api) => {
  const s = await createServer({
    configFile: new URL('../vite.config.ts', import.meta.url).pathname,
    server: { port, strictPort: true, proxy: { '/api': api, '/previews': api, '/library-file': api } },
    logLevel: 'error',
  });
  await s.listen();
  return s;
};
const servers = [await vite(5191, 'http://127.0.0.1:7871'), await vite(5192, 'http://127.0.0.1:7872')];
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const VIEW = { width: 1440, height: 900 };
const stills = await (await browser.newContext({ viewport: VIEW, deviceScaleFactor: 2 })).newPage();
const wait = (p, ms) => p.waitForTimeout(ms);
const go = async (p, hash, port = 5191) => {
  await p.goto(`http://localhost:${port}/?library=live&chrome=mac${hash}`);
  await wait(p, 1500);
};
const shot = async (name) => {
  await wait(stills, 800);
  await stills.screenshot({ path: `${out}${name}.png` });
  console.log(`shot ${name}`);
};
// A clip is its own context so Playwright records just that flow; the WebM is named on close.
const clip = async (name, port, hash, run) => {
  const ctx = await browser.newContext({ viewport: VIEW, recordVideo: { dir: tmp, size: VIEW } });
  const p = await ctx.newPage();
  await go(p, hash, port);
  await run(p);
  const video = p.video();
  await ctx.close();
  renameSync(await video.path(), `${out}${name}.webm`);
  console.log(`clip ${name}`);
};

try {
  if (want('grid')) {
    await go(stills, '#/cull');
    await stills.waitForSelector('.tile img');
    await shot('grid');
    await stills.keyboard.press('n');
    await stills.waitForSelector('.burst-cell');
    await shot('stacks');
  }
  if (want('edit')) {
    await go(stills, `#/edit/${editId}`);
    await stills.getByTestId('take').first().waitFor({ timeout: 60000 });
    await stills.getByTestId('take').nth(1).click();
    await wait(stills, 2500);
    await shot('edit');
    // Presets on the plain Photo, with a Preset hovered to preview it on the image.
    await go(stills, `#/edit/${editId}`);
    await stills.keyboard.press('Shift+P');
    await wait(stills, 2000);
    await stills.locator('.preset', { hasText: 'Moody' }).first().hover();
    await wait(stills, 2500);
    await shot('presets');
  }
  if (want('import')) {
    await fetch('http://127.0.0.1:7872/api/set_queue', { method: 'POST', body: JSON.stringify({ paused: true }) });
    await clip('import', 5192, '#/import', async (p) => {
      await p.getByTestId('import-tile').first().waitFor({ timeout: 30000 });
      for (let i = 0; i < 50 && !/Coast-Trip/.test(await p.getByTestId('import-name').inputValue()); i++) await wait(p, 200);
      for (const id of ['import-auto-cull', 'import-suggest']) {
        const box = p.getByTestId(id);
        if ((await box.getAttribute('data-state')) === 'checked') await box.click();
      }
      await wait(p, 1800);
      await p.getByTestId('import-start').click();
      await p.waitForSelector('.tile img', { timeout: 60000 });
      await wait(p, 5000);
    });
  }
  if (want('takes')) {
    await clip('takes', 5191, `#/edit/${editId}`, async (p) => {
      await p.getByTestId('take').first().waitFor({ timeout: 60000 });
      await wait(p, 1500);
      for (let i = 0; i < 3; i++) {
        await p.keyboard.press('Tab');
        await wait(p, 2200);
      }
      await p.keyboard.press('Enter');
      await wait(p, 2500);
      // Hold \ for the before, like Lightroom.
      await p.keyboard.down('Backslash');
      await wait(p, 1600);
      await p.keyboard.up('Backslash');
      await wait(p, 1800);
    });
  }
  if (want('agent')) {
    await go(stills, `#/edit/${agentId}`);
    await stills.getByPlaceholder(/A bit more on the face/).fill('Fix the lighting on this one');
    await stills.getByRole('button', { name: 'Send' }).click();
    await stills.locator('.agent').getByRole('button', { name: 'Undo', exact: true }).waitFor({ timeout: 300000 });
    await wait(stills, 3000);
    await shot('agent');
  }
} finally {
  await browser.close();
  for (const s of servers) await s.close();
  main.kill();
  fresh.kill();
  mock.close();
}
