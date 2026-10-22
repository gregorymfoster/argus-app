// Shots of the People view: everyone named and the groups to name, a name being typed with
// its autocomplete, and one person's page with "Is this …?". Dummy data by default; pass a
// live URL (`?library=live`, with `argus serve` behind it) to shoot a real Library.
//
//   node scripts/people-shots.mjs [OUTDIR] [query]
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const out = process.argv[2] ?? 'out/people';
const query = process.argv[3] ?? '?chrome=mac';
mkdirSync(out, { recursive: true });
const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const p = await ctx.newPage();
const errors = [];
p.on('pageerror', (e) => errors.push(String(e)));
const fail = (m) => {
  console.error(`people-shots: ${m}`);
  process.exitCode = 1;
};

await p.goto(`http://localhost:5199/${query}#/people`);
await p.waitForSelector('[data-testid=person-tile], [data-testid=face-group]', { timeout: 20000 });
await p.waitForTimeout(1500);
await p.screenshot({ path: `${out}/people.png` });
const groups = await p.locator('[data-testid=face-group]').count();
const named = await p.locator('[data-testid=person-tile]').count();
console.log(`people-shots: ${named} named, ${groups} groups to name`);

// Type a name into the first group: the menu offers people already named, then New.
if (groups) {
  const field = p.locator('[data-testid=name-field]').first();
  await field.click();
  const first = named ? (await p.locator('[data-testid=person-tile] .people-name').first().textContent()) : 'Sam';
  await field.type(first.slice(0, 2));
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${out}/people-naming.png` });
  await field.press('Escape');
}

// Name the biggest group "Sam": it becomes a person and leaves the groups.
if (groups) {
  const field = p.locator('[data-testid=name-field]').first();
  await field.click();
  await field.fill('Sam');
  await field.press('ArrowDown');
  await field.press('Enter');
  await p.waitForTimeout(800);
  const names = await p.locator('[data-testid=person-tile] .people-name').allTextContents();
  if (!names.includes('Sam')) fail(`naming a group didn't make a person: ${names.join(', ')}`);
}

// A person's page.
if (await p.locator('[data-testid=person-tile]').count()) {
  // The person with faces still to confirm, when there is one (Maya in the dummy data).
  const maya = p.locator('[data-testid=person-tile]', { hasText: 'Maya' });
  await ((await maya.count()) ? maya.first() : p.locator('[data-testid=person-tile]').first()).click();
  await p.waitForTimeout(1500);
  await p.screenshot({ path: `${out}/person.png` });
  if (!(await p.locator('.people-photo').count())) fail('the person page shows no Photos');
}

if (errors.length) fail(`page errors: ${errors.join(' | ')}`);
await browser.close();
await server.close();
