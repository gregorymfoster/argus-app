// Design pass v2 screens (Import, Importing, Cull and its menus, Background work, Settings ›
// AI and Compute) on a real engine and a fake card, at the mock's 1500×940, so each one
// can be laid beside its mock board. Shots in out/v2/ (or the folder given).
//
//   node scripts/v2-shots.mjs [out-dir]

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = new URL('../../', import.meta.url).pathname;
const rawDir = [join(root, 'fixtures/raw-a7rv'), join(root, 'fixtures/raw')].find((d) => existsSync(d) && readdirSync(d).length);
const bin = process.env.ARGUS_BIN ?? join(root, 'target/fast/argus');
const out = process.argv[2] ?? new URL('../out/v2/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const tmp = mkdtempSync(join(tmpdir(), 'argus-v2-'));
const volumes = join(tmp, 'Volumes');
const dcim = join(volumes, 'Untitled', 'DCIM', '100MSDCF');
mkdirSync(dcim, { recursive: true });
for (const f of readdirSync(rawDir).filter((f) => /\.arw$/i.test(f))) copyFileSync(join(rawDir, f), join(dcim, f));

// A stand-in Ollama: names Shoots, lists two installed models, answers anything else empty.
const mock = httpServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  if (req.url === '/api/tags') return res.end(JSON.stringify({ models: [{ name: 'qwen3.8:27b', size: 17.3e9 }, { name: 'gemma4:12b', size: 8.1e9 }] }));
  if (req.url === '/api/ps') return res.end(JSON.stringify({ models: [{ name: 'qwen3.8:27b', size_vram: 19e9 }] }));
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const r = JSON.parse(body || '{}');
    if (!r.messages?.length) return res.end(JSON.stringify({ message: { content: '' }, done: true }));
    setTimeout(() => res.end(JSON.stringify({ message: { content: '{"name":"Fall Farm"}' }, done: true })), 300);
  });
});
await new Promise((r) => mock.listen(0, r));
const serve = spawn(bin, ['serve', '--library', join(tmp, 'lib'), '--port', '7879'], {
  env: { ...process.env, ARGUS_VOLUMES: volumes, OLLAMA_HOST: `127.0.0.1:${mock.address().port}`, ARGUS_PROVIDER: 'ollama' },
  stdio: ['ignore', 'ignore', 'inherit'],
});
const api = (cmd, args = {}) => fetch(`http://127.0.0.1:7879/api/${cmd}`, { method: 'POST', body: JSON.stringify(args) }).then((r) => r.json());
const vite = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5197, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7879', '/previews': 'http://127.0.0.1:7879' } }, logLevel: 'error' });
await vite.listen();
const browser = await chromium.launch();
const errors = [];
const shot = async (p, name) => p.screenshot({ path: join(out, `${name}.png`) });
try {
  for (let i = 0; i < 100 && !(await api('list_shoots').then(() => true, () => false)); i++) await new Promise((r) => setTimeout(r, 200));
  await api('set_queue', { paused: true });
  const p = await (await browser.newContext({ viewport: { width: 1500, height: 940 } })).newPage();
  p.on('pageerror', (e) => errors.push(e.message));
  await p.goto('http://localhost:5197/?library=live#/import');
  await p.getByTestId('import-tile').first().waitFor({ timeout: 30000 });
  await p.waitForTimeout(2500);
  await shot(p, 'V2-Import');
  await p.getByTestId('import-source').click();
  await p.waitForTimeout(300);
  await shot(p, 'V2-Import-from');
  await p.keyboard.press('Escape');
  await p.getByTestId('import-start').click();
  for (const t0 = Date.now(); Date.now() - t0 < 60000 && !p.url().includes('#/cull'); ) await p.waitForTimeout(100);
  await p.waitForTimeout(600);
  await shot(p, 'V2-Importing');
  for (const t0 = Date.now(); Date.now() - t0 < 120000 && (await api('import_result')).running; ) await p.waitForTimeout(300);
  await p.goto('http://localhost:5197/?library=live#/cull');
  await p.waitForTimeout(2500);
  await shot(p, 'V2-Cull');
  await p.locator('.tile').nth(1).click({ button: 'right' });
  await p.waitForTimeout(300);
  await shot(p, 'V2-Cull-menu');
  await p.keyboard.press('Escape');
  await p.locator('[data-testid^="shoot-"]').first().click({ button: 'right' });
  await p.waitForTimeout(300);
  await shot(p, 'V2-Shoot-menu');
  await p.keyboard.press('Escape');
  await p.locator('[data-testid^="shoot-"]').first().dblclick();
  await p.waitForTimeout(300);
  await shot(p, 'V2-Shoot-rename');
  await p.keyboard.press('Escape');
  // Something in the queue to show: Suggestions for the Shoot, held paused.
  await api('enqueue_job', { kind: 'suggest_shoot', args: {} }).catch(() => {});
  await p.waitForTimeout(1500);
  await p.getByTestId('queue-progress').click({ timeout: 5000 }).catch((e) => errors.push(`queue: ${e.message.split('\n')[0]}`));
  await p.waitForTimeout(600);
  await shot(p, 'V2-Queue');
  await p.keyboard.press('Escape');
  await p.keyboard.press('Control+Comma');
  await p.getByTestId('settings-ai').click();
  await p.waitForTimeout(800);
  await shot(p, 'V2-Settings');
} catch (e) {
  errors.push(String(e).split('\n')[0]);
} finally {
  await browser.close();
  await vite.close();
  serve.kill();
  mock.close();
}
if (errors.length) console.error(errors.join('\n'));
console.log('shots in', out);
