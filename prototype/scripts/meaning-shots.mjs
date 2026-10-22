// Screenshots of Settings › AI › Search on dummy data: the Download row, then its progress
// bar with an ETA, then Installed. node scripts/meaning-shots.mjs
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
const p = await (await browser.newContext({ viewport: { width: 1280, height: 860 }, deviceScaleFactor: 2 })).newPage();
p.on('pageerror', (e) => errors.push(e.message));
await p.goto('http://localhost:5198/#/');
await p.waitForTimeout(800);
await p.evaluate(() => window.dispatchEvent(new CustomEvent('argus:settings', { detail: 'ai' })));
const row = p.getByTestId('settings-meaning');
await row.scrollIntoViewIfNeeded();
await p.screenshot({ path: `${out}meaning-download.png` });
await row.click();
await p.waitForTimeout(2600);
if (!(await p.getByTestId('meaning-progress').count())) errors.push('no progress bar');
await p.screenshot({ path: `${out}meaning-progress.png` });
await p.waitForTimeout(10000);
if (!(await p.getByText('Installed', { exact: false }).count())) errors.push('never installed');
await p.screenshot({ path: `${out}meaning-installed.png` });
await browser.close();
await server.close();
if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`shots in ${out}`);
