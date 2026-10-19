// Screenshots of the Edit screen's right panel at two window widths, for checking the
// Lightroom-style panel without a human: node scripts/edit-panel-shots.mjs [prefix]
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const prefix = process.argv[2] ?? 'edit-panel';
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
/** Open or close an Edit panel section by its id. */
const sec = (p, id) => p.locator(`[data-testid=section-${id}] .dev-section-toggle`).click();
const steps = [
  ['', async () => {}],
  ['-auto', async (p) => (await p.getByRole('button', { name: /^Auto/ }).count()) && p.getByRole('button', { name: /^Auto/ }).click()],
  ['-presets', async (p) => {
    await p.getByRole('tab', { name: 'Presets' }).click();
    await p.waitForTimeout(300);
    await p.getByTestId('preset-moody').hover();
  }],
  ['-masking', async (p) => (await p.getByRole('tab', { name: 'Masking' }).count()) && p.getByRole('tab', { name: 'Masking' }).click()],
  ['-recommended', async (p) => {
    await p.getByTestId('rec-warm-film').click();
    await p.getByTestId('fix-faces').click();
  }],
  ['-crop', async (p) => {
    await p.getByRole('tab', { name: 'Crop' }).click();
    await p.getByRole('radio', { name: '4:5' }).click();
  }],
  ['-people', async (p) => {
    await p.getByRole('tab', { name: 'Masking' }).click();
    await p.getByTestId('mask-create').getByRole('button', { name: 'People' }).click();
  }],
  ['-radial', async (p) => {
    await p.getByRole('tab', { name: 'Masking' }).click();
    await p.getByTestId('mask-create').getByRole('button', { name: 'Radial' }).click();
  }],
  ['-curve', async (p) => {
    await sec(p, 'light');
    await sec(p, 'curve');
    await p.getByRole('radio', { name: 'Point curve' }).click();
    await p.getByTestId('curve-graph').scrollIntoViewIfNeeded();
    const g = await p.getByTestId('curve-graph').boundingBox();
    await p.mouse.move(g.x + g.width * 0.25, g.y + g.height * 0.75);
    await p.mouse.down();
    await p.mouse.move(g.x + g.width * 0.25, g.y + g.height * 0.82, { steps: 4 });
    await p.mouse.up();
  }],
  ['-grading', async (p) => {
    await sec(p, 'light');
    await sec(p, 'color');
    await sec(p, 'grading');
    await p.getByTestId('grading-wheels').scrollIntoViewIfNeeded();
    const w = await p.getByRole('slider', { name: 'Shadows hue and saturation' }).boundingBox();
    await p.mouse.click(w.x + w.width * 0.3, w.y + w.height * 0.2);
  }],
  ['-effects', async (p) => {
    await sec(p, 'light');
    await sec(p, 'color');
    await sec(p, 'effects');
    await sec(p, 'detail');
  }],
  ['-optics', async (p) => {
    await sec(p, 'light');
    await sec(p, 'color');
    await sec(p, 'optics');
    await sec(p, 'calibration');
  }],
  ['-point', async (p) => {
    await sec(p, 'light');
    await sec(p, 'mixer');
    await p.getByRole('radio', { name: 'Shift one exact colour picked from the photo' }).click();
    await p.getByTestId('point-pick').click();
    const s = await p.getByTestId('pick-overlay').boundingBox();
    await p.mouse.click(s.x + s.width * 0.5, s.y + s.height * 0.45);
  }],
  // New takes (R): queued, then a stage with a bar, ETA and Cancel over developing tiles, then they land.
  ['-takes-queued', async (p) => {
    await p.keyboard.press('r');
    await p.getByTestId('takes-progress').waitFor();
    if ((await p.getByTestId('take-pending').count()) !== 3) throw new Error('takes-queued: expected 3 pending tiles');
  }],
  ['-takes-running', async (p) => {
    await p.keyboard.press('r');
    await p.waitForTimeout(5000);
    const line = await p.getByTestId('takes-progress').innerText();
    if (!/Model thinking/.test(line) || !/left/.test(line)) throw new Error(`takes-running: ${line}`);
  }],
  ['-takes-landed', async (p) => {
    await p.keyboard.press('r');
    await p.getByTestId('takes-progress').waitFor({ state: 'detached', timeout: 15000 });
    if (await p.getByTestId('take-pending').count()) throw new Error('takes-landed: pending tiles left');
    if ((await p.getByTestId('take').count()) < 4) throw new Error('takes-landed: takes missing');
  }],
  // A take's Amount (0-150%, like a Lightroom preset's): shown while previewing, arrows move it.
  ['-take-amount', async (p) => {
    await p.keyboard.press('Tab');
    const amount = p.getByTestId('take-amount');
    await amount.waitFor();
    await amount.getByRole('slider').focus();
    for (let i = 0; i < 6; i++) await p.keyboard.press('ArrowLeft');
    const label = await amount.locator('xpath=..').innerText();
    if (!/70%/.test(label)) throw new Error(`take-amount: ${label}`);
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
    await p.goto('http://localhost:5198/#/edit/p15');
    await p.waitForTimeout(800);
    await act(p);
    await p.waitForTimeout(400);
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
