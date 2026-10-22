// Screenshots of the loupe's Caption and Keywords on dummy data, and the keyboard flow
// they promise (K adds with autocomplete, a click keeps a suggestion, ⇧↵ keeps them all):
// node scripts/keyword-shots.mjs [prefix]. ONLY=add picks one step; widths 1500 and 1280.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const prefix = process.argv[2] ?? 'keywords';
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
const fail = (m) => errors.push(m);
const suggested = (p) => p.locator('[data-testid=kw-chip][data-suggested]').count();
const scrollToTags = (p) => p.getByTestId('keywords').evaluate((e) => e.scrollIntoView({ block: 'center' }));
const steps = [
  ['', async (p) => {
    await scrollToTags(p);
    if (!(await suggested(p))) fail('no AI suggestions shown');
  }],
  ['-add', async (p) => {
    await p.keyboard.press('k');
    await p.keyboard.type('hik');
    await p.waitForTimeout(200);
    await scrollToTags(p);
    const opts = await p.getByRole('option').allTextContents();
    if (!opts.some((o) => /hiking/.test(o))) fail(`autocomplete: ${opts.join(' | ')}`);
    if (!opts.some((o) => /New “hik”/.test(o))) fail('no New keyword option');
  }],
  ['-added', async (p) => {
    await p.keyboard.press('k');
    await p.keyboard.type('Places > Portugal > Lisbon');
    await p.keyboard.press('ArrowDown');
    await p.keyboard.press('Enter');
    await p.keyboard.press('Escape');
    await scrollToTags(p);
    if (!(await p.getByText('Portugal › Lisbon').count())) fail('Lisbon not added');
  }],
  ['-accept', async (p) => {
    const before = await suggested(p);
    await p.locator('[data-testid=kw-chip][data-suggested] .kw-keep').first().click();
    if ((await suggested(p)) !== before - 1) fail('click did not keep one suggestion');
    await p.keyboard.press('Shift+Enter');
    await scrollToTags(p);
    if (await suggested(p)) fail('⇧↵ left suggestions');
    if (await p.getByTestId('accept-all').count()) fail('Accept all still shown');
  }],
];
const only = process.env.ONLY;
for (const width of [1500, 1280]) {
  if (only && width !== 1500) continue;
  for (const [suffix, act] of steps) {
    if (only && !suffix.includes(only)) continue;
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, deviceScaleFactor: 2 });
    const p = await ctx.newPage();
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto('http://localhost:5199/#/loupe/p15');
    await p.waitForTimeout(800);
    await act(p);
    await p.waitForTimeout(300);
    await p.screenshot({ path: `${out}${prefix}-${width}${suffix}.png` });
    await ctx.close();
  }
}
await browser.close();
await server.close();
if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`shots in ${out}`);
