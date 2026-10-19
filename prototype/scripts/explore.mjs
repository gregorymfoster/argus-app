// Bug-bash crawl: every main screen of the prototype on a real Library, with no model
// running (Ollama points at a dead port, the Queue is off), so the AI-unavailable paths
// get driven too. Fails on page errors, console errors and engine calls that error;
// writes out/shots/explore-*.png at 1280 px to look at. `node scripts/explore.mjs <library-dir>`.
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/explore.mjs <library-dir>');
  process.exit(2);
}
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-explore-')), 'lib');
cpSync(source, library, { recursive: true });

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7868'], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, ARGUS_PROVIDER: 'ollama', OLLAMA_HOST: '127.0.0.1:9', ARGUS_QUEUE: 'off' },
});
const api = (p) => fetch(`http://127.0.0.1:7868${p}`).then((r) => r.json());
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
  server: { port: 5188, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7868', '/previews': 'http://127.0.0.1:7868', '/library-file': 'http://127.0.0.1:7868' } },
  logLevel: 'error',
});
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const problems = [];
let step = 'start';
const note = (m) => (problems.push(`[${step}] ${m}`), console.log(`FAIL [${step}] ${m}`));

try {
  const shoots = await api('/api/list_shoots');
  const photos = await api(`/api/photos?shoot_id=${shoots[shoots.length - 1].id}`);
  const ids = photos.map((p) => p.id);
  const p = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  p.on('pageerror', (e) => note(`page error: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && !/Failed to load resource/.test(m.text()) && note(`console: ${m.text().slice(0, 200)}`));
  p.on('response', async (r) => {
    if (!r.url().includes('/api/') || r.ok()) return;
    note(`${r.status()} ${new URL(r.url()).pathname}: ${(await r.text().catch(() => '')).slice(0, 160)}`);
  });
  const at = async (name, hash, then) => {
    step = name;
    if (hash) await p.goto(`http://localhost:5188/?library=live${hash}`);
    await p.waitForTimeout(900);
    if (then) await then();
    await p.screenshot({ path: `${out}explore-${name}.png` });
    console.log(`ok   ${name}`);
  };

  await at('cull', '#/cull');
  await at('cull-select-all', null, async () => {
    await p.locator('.tile').first().click();
    await p.keyboard.press('ControlOrMeta+a');
  });
  await at('palette', null, async () => {
    await p.keyboard.press('Escape');
    await p.keyboard.press('ControlOrMeta+k');
    await p.keyboard.type('auto');
  });
  await p.keyboard.press('Enter'); // Auto every Photo: no model needed
  await at('auto-all', null, async () => {
    // The grid shows the edit: edited tiles swap their preview for the engine's edited preview.
    await p.waitForFunction(() => document.querySelectorAll('.tile img[src*="previews/edited/"]').length > 0, null, { timeout: 20000 }).catch(() => note('no tile shows its Auto edit'));
  });
  // An AI cull with no model running must end in a plain message, not a hang.
  await at('cull-no-model', null, async () => {
    const box = p.locator('textarea').last();
    await box.fill('Cull this shoot');
    await box.press('Enter');
    await p.waitForTimeout(8000);
  });
  // The Queue chip only shows while something is queued or running.
  await at('queue', null, async () => (await p.locator('[data-testid="queue"]').count()) && p.locator('[data-testid="queue"]').first().click());
  await p.keyboard.press('Escape');
  await at('loupe', `#/loupe/${ids[0]}`);
  await at('compare', `#/compare/${ids.slice(0, 2).join(',')}`);
  await at('edit', `#/edit/${ids[0]}`);
  // Open every collapsed section of the Edit panel, then nudge a slider with the keyboard.
  await at('edit-sections', null, async () => {
    for (const h of await p.locator('[data-testid="edit-panel"] [aria-expanded="false"]').all()) await h.click().catch(() => {});
    const slider = p.locator('[data-testid="edit-panel"] [role="slider"]').first();
    if (await slider.count()) {
      await slider.focus();
      await p.keyboard.press('ArrowRight');
    }
  });
  await at('look', '#/look');
  await at('import', '#/import');
  for (const s of ['general', 'library', 'import', 'export', 'ai', 'updates', 'keys']) {
    await at(`settings-${s}`, '#/cull', async () => {
      await p.evaluate((id) => window.dispatchEvent(new CustomEvent('argus:settings', { detail: id })), s);
      await p.waitForTimeout(500);
    });
  }
  await at('settings-ai-key', null, async () => {
    await p.locator('[data-testid="settings-ai"]').click();
    await p.locator('[data-testid="settings-claude-api"]').click();
    await p.locator('[data-testid="settings-api-key"]').fill('not-a-key');
    await p.keyboard.press('Enter');
    await p.waitForTimeout(400);
  });
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
// The bad API key and the cull with no model are meant to fail (and say so on screen).
const expected = (m) => (m.includes('[settings-ai-key]') && m.includes('set_api_key')) || (m.includes('[cull-no-model]') && m.includes('cull_shoot'));
const real = problems.filter((m) => !expected(m));
console.log(real.length ? `\n${real.length} problem(s)` : '\nexplore: no problems');
process.exit(real.length ? 1 : 0);
