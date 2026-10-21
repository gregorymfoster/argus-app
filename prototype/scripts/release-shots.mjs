// Screenshots of the real app for the public release page (Greg, 2026-10-20): the grid after
// an AI cull, the Stacks view, Edit with its Suggestions and an Agent edit, in the Mac window
// chrome at 2x. Works on a copy of the Library, which must hold only public-domain photos
// (see docs/release.md): never point it at Greg's Library or the bench.
//
//   node scripts/release-shots.mjs <library-dir> <edit-photo-id> <agent-photo-id>
//
// The Agent request runs for real, so set ARGUS_PROVIDER (claude-code in the cloud).
// JPEGs land in out/release-shots/.
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [source, editId, agentId] = process.argv.slice(2);
if (!agentId) {
  console.error('usage: node scripts/release-shots.mjs <library-dir> <edit-photo-id> <agent-photo-id>');
  process.exit(2);
}
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/release-shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const library = join(mkdtempSync(join(tmpdir(), 'argus-release-shots-')), 'lib');
cpSync(source, library, { recursive: true });

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7869'], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, ARGUS_QUEUE: 'off' },
});
for (let i = 0; ; i++) {
  try {
    await fetch('http://127.0.0.1:7869/api/list_shoots');
    break;
  } catch (e) {
    if (i > 50) throw e;
    await new Promise((r) => setTimeout(r, 100));
  }
}
const server = await createServer({
  configFile: new URL('../vite.config.ts', import.meta.url).pathname,
  server: { port: 5189, strictPort: true, proxy: { '/api': 'http://127.0.0.1:7869', '/previews': 'http://127.0.0.1:7869', '/library-file': 'http://127.0.0.1:7869' } },
  logLevel: 'error',
});
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const p = await (await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 })).newPage();
const go = async (hash) => {
  await p.goto(`http://localhost:5189/?library=live&chrome=mac${hash}`);
  await p.waitForTimeout(1500);
};
const shot = async (name) => {
  await p.waitForTimeout(800);
  await p.screenshot({ path: `${out}${name}.jpg`, type: 'jpeg', quality: 86 });
  console.log(`shot ${name}`);
};

try {
  await go('#/cull');
  await p.waitForSelector('.tile img');
  await shot('1-grid');
  await p.keyboard.press('n');
  await p.waitForSelector('.burst-cell');
  await shot('2-stacks');
  await p.keyboard.press('Escape');

  await go(`#/edit/${editId}`);
  await shot('3-edit');

  await go(`#/edit/${agentId}`);
  await p.getByPlaceholder(/A bit more on the face/).fill('Fix the lighting on this one');
  await p.getByRole('button', { name: 'Send' }).click();
  await p.locator('.agent').getByRole('button', { name: 'Undo', exact: true }).waitFor({ timeout: 300000 });
  await p.waitForTimeout(3000); // the edited render lands
  await shot('4-agent');
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
}
