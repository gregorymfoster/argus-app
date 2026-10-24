// Screenshots every screen and runs smoke checks through the real UI,
// checks, so an agent can verify UI changes without a human.
//
//   node scripts/shots.mjs              all shots
//   node scripts/shots.mjs cull look    only names containing these words

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';

const filters = process.argv.slice(2);
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5199, strictPort: true }, logLevel: 'error' });
await server.listen();
const base = 'http://localhost:5199/';

const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const errors = [];
const results = [];

// `zoom` renders as the app's View › Zoom would (prototype/src/ui/zoom.ts): a webview zoomed
// to z lays out like a viewport 1/z the size at z times the pixel density.
async function page(path, variants = '', zoom = 1) {
  const ctx = await browser.newContext({ viewport: { width: Math.round(1440 / zoom), height: Math.round(900 / zoom) }, deviceScaleFactor: zoom });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(`${path}: ${e.message}`));
  p.on('console', (m) => m.type() === 'error' && !/fonts\.g/.test(m.text()) && errors.push(`${path}: ${m.text()}`));
  const q = variants ? `?v=${variants}` : '';
  await p.goto(`${base}${q}${path}`);
  await p.waitForTimeout(700);
  return { p, ctx };
}

async function shot(name, path, { variants = '', act, zoom } = {}) {
  if (filters.length && !filters.some((f) => name.includes(f))) return;
  const { p, ctx } = await page(path, variants, zoom);
  if (act) await act(p);
  // Settled, so two runs of one commit shoot the same pixels (shot-diff.mjs): the grid paints
  // its stand-in previews on idle callbacks, so wait until no new image appears and all decoded.
  for (let last = -1, i = 0; i < 20; i++) {
    const n = await p.evaluate(() => (document.images.length * 2 + [...document.images].filter((x) => x.complete).length));
    if (n === last) break;
    last = n;
    await p.waitForTimeout(250);
  }
  await p.screenshot({ path: `${out}${name}.png`, animations: 'disabled', caret: 'hide' });
  results.push(name);
  await ctx.close();
}

// Screens
// The desktop app's Mac chrome (overlay title bar, translucent sidebar), previewed in the browser.
await shot('00-mac-chrome-cull', '?chrome=mac#/cull', { act: (p) => p.waitForTimeout(600) });
await shot('00b-mac-chrome-edit', '?chrome=mac#/edit/p15', { act: (p) => p.waitForTimeout(600) });
await shot('01-import-dialog', '#/import');
await shot('02-import-started', '#/import', {
  act: async (p) => {
    await p.getByRole('button', { name: 'Import 112 photos' }).click();
    await p.waitForTimeout(1200);
  },
});
await shot('03-cull', '#/cull', { act: (p) => p.waitForTimeout(600) });
await shot('03b-cull-hover-flags', '#/cull', {
  act: async (p) => {
    await p.locator('.tile').nth(2).hover();
    await p.waitForTimeout(300);
  },
});
await shot('03c-cull-dense', '#/cull', {
  act: async (p) => {
    for (let i = 0; i < 3; i++) await p.keyboard.press('Minus');
    await p.waitForTimeout(600);
  },
});
await shot('03d-cull-context-menu', '#/cull', {
  act: async (p) => {
    await p.locator('.tile').nth(1).click({ button: 'right' });
    await p.waitForTimeout(300);
  },
});
await shot('03d2-shoot-context-menu', '#/cull', {
  act: async (p) => {
    await p.getByTestId('shoot-tahoe').click({ button: 'right' });
    await p.waitForTimeout(300);
  },
});
await shot('03d3-shoot-rename', '#/cull', {
  act: async (p) => {
    await p.getByTestId('shoot-tahoe').dblclick();
    await p.keyboard.type('Boat tour');
    await p.waitForTimeout(200);
  },
});
await shot('03d4-cull-pinched-large', '#/cull', {
  act: async (p) => {
    await p.mouse.move(700, 400);
    await p.keyboard.down('Control');
    for (let i = 0; i < 6; i++) await p.mouse.wheel(0, -40);
    await p.keyboard.up('Control');
    await p.waitForTimeout(600);
  },
});
await shot('03e-agent-provider-menu', '#/cull', {
  act: async (p) => {
    await p.getByTestId('provider').click();
    await p.waitForTimeout(300);
    // Each option's title and note stay inside its own row (they overlapped, Greg 2026-10-18).
    const rows = await p.locator('[role=menuitem]').evaluateAll((els) => els.map((e) => [e.getBoundingClientRect(), e.scrollHeight - e.clientHeight]));
    rows.forEach(([r, spill], i) => {
      if (spill > 0) throw new Error(`provider option ${i} spills out of its row`);
      if (i && r.top < rows[i - 1][0].bottom - 0.5) throw new Error(`provider option ${i} overlaps the one above`);
    });
  },
});
await shot('03f-zoom-80', '#/cull', { zoom: 0.8 });
await shot('03g-zoom-125', '#/cull', { zoom: 1.25 });
await shot('03h-zoom-125-edit', '#/edit/p15', { zoom: 1.25 });
await shot('03e-export-dialog', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Meta+e');
    await p.keyboard.press('Control+e');
    await p.waitForTimeout(400);
  },
});
await shot('03f-export-running', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Control+e');
    await p.getByRole('button', { name: /^Web/ }).click();
    await p.getByTestId('export-start').click();
    await p.waitForTimeout(1200);
  },
});
await shot('04-loupe', '#/loupe/p5');
await shot('05-compare', '#/cull', {
  act: async (p) => {
    await p.locator('.tile:has(.badge)').first().click();
    await p.keyboard.press('c');
    await p.waitForTimeout(900);
  },
});
await shot('05b-compare-ai-prefers', '#/cull', {
  act: async (p) => {
    await p.locator('.tile:has(.badge)').first().click();
    await p.keyboard.press('c');
    await p.waitForTimeout(500);
    await p.keyboard.press('ArrowUp');
    await p.waitForTimeout(600);
  },
});
await shot('05c-bursts', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('n');
    await p.waitForTimeout(900);
  },
});
await shot('05d-bursts-kept-two', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('n');
    await p.waitForTimeout(500);
    await p.keyboard.press('Enter');
    await p.waitForTimeout(400);
    await p.keyboard.press('p');
    await p.keyboard.press('ArrowRight');
    await p.keyboard.press('p');
    await p.waitForTimeout(600);
  },
});
await shot('06-look-empty', '#/look');
await shot('07-look-applied', '#/look', {
  act: async (p) => {
    await p.getByRole('button', { name: /Warm, soft/ }).click();
    await p.waitForTimeout(2600);
  },
});
await shot('08-edit', '#/edit/p15', {
  act: async (p) => {
    // Under Edit the Agent shows its suggestions once the composer has focus.
    await p.getByPlaceholder(/A bit more on the face/).click();
    await p.getByRole('button', { name: /Brighten her face/ }).click();
    await p.waitForTimeout(3200);
    await p.keyboard.press('o');
    await p.waitForTimeout(300);
  },
});
await shot('08b-edit-suggestions', '#/edit/p15', {
  act: async (p) => {
    await p.keyboard.press('Tab');
    await p.keyboard.press('Tab');
    await p.waitForTimeout(300);
  },
});
await shot('09b-palette-ask', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Control+k');
    await p.keyboard.type('which photos have closed eyes');
    await p.waitForTimeout(300);
  },
});
await shot('09-palette', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Control+k');
    await p.keyboard.type('look');
    await p.waitForTimeout(300);
  },
});
await shot('10-settings-library', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Control+Comma');
    await p.getByTestId('settings-library').click();
    await p.getByTestId('library-settings').waitFor();
    await p.waitForTimeout(300);
  },
});
for (const id of ['general', 'import', 'export']) {
  await shot(`10-settings-${id}`, '#/cull', {
    act: async (p) => {
      await p.keyboard.press('Control+Comma');
      await p.getByTestId(`settings-${id}`).click();
      await p.getByTestId(`settings-${id}-page`).waitFor();
      await p.waitForTimeout(300);
    },
  });
}
await shot('12-best-photos', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('b');
    await p.getByTestId('best-photos').waitFor();
    await p.waitForTimeout(300);
  },
});
await shot('13-stars-filter', '#/cull', {
  act: async (p) => {
    await p.getByTestId('stars-filter').click();
    await p.waitForTimeout(300);
  },
});
await shot('13b-sort-menu', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Shift+S');
    await p.getByTestId('sort').click();
    await p.waitForTimeout(300);
  },
});
// Learn as you cull: after a few Picks and Rejects the AI's stars refresh (data/learn.ts).
{
  await shot('13c-learn-as-you-cull', '#/cull', {
    act: async (p) => {
      // Flag among the Unflagged, where every press is the person's own call.
      await p.getByTitle('Unflagged').click();
      for (let i = 0; i < 14; i++) {
        await p.keyboard.press(i % 2 ? 'x' : 'p');
        await p.waitForTimeout(60);
      }
      await p.getByTitle('All').click();
      await p.keyboard.press('Shift+S');
      await p.waitForTimeout(1500);
    },
  });
}
// Edit as you cull: Keepers first re-orders the frames ahead as you flag (data/keepers.ts), and a
// hand edit gets the Picks and likely keepers their Shoot match (data/preEdit.ts).
const flagSome = async (p, n = 12) => {
  for (let i = 0; i < n; i++) {
    await p.keyboard.press(i % 3 ? 'p' : 'x');
    await p.waitForTimeout(240);
  }
  await p.waitForTimeout(1600);
};
await shot('13d-keepers-first-loupe', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Shift+K');
    await p.keyboard.press('Enter');
    await p.waitForTimeout(400);
    await flagSome(p);
  },
});
await shot('13e-keepers-first-menu', '#/cull', {
  act: async (p) => {
    await p.keyboard.press('Shift+K');
    await p.getByTestId('sort').click();
    await p.waitForTimeout(300);
  },
});
const handEdit = async (p) => {
  await p.goto(`${base}#/edit/p15`);
  await p.waitForTimeout(500);
  await p.getByRole('slider', { name: 'Temp' }).first().focus();
  for (let i = 0; i < 5; i++) await p.keyboard.press('ArrowRight');
  await p.waitForTimeout(300);
};
await shot('13f-pre-edits-grid', '#/cull', {
  act: async (p) => {
    await handEdit(p);
    await p.goto(`${base}#/cull`);
    await p.waitForTimeout(600);
  },
});
await shot('13g-pre-edit-loupe', '#/cull', {
  act: async (p) => {
    await handEdit(p);
    await p.goto(`${base}#/cull`);
    await p.waitForTimeout(400);
    await p.getByTestId('pre-edits-show').click();
    await p.keyboard.press('Enter');
    await p.waitForTimeout(400);
    await p.keyboard.press('Backslash');
    await p.waitForTimeout(600);
  },
});
await shot('14-cull-hover-stars', '#/cull', {
  act: async (p) => {
    await p.locator('.tile').nth(1).locator('.stars button').nth(3).hover();
    await p.waitForTimeout(300);
  },
});

// First launch in the app: no Library yet, so it asks where to make one (the engine is faked).
if (!filters.length || filters.some((f) => 'first-run'.includes(f))) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const p = await ctx.newPage();
  p.on('pageerror', (e) => errors.push(`first-run: ${e.message}`));
  await p.route('**/api/library', (r) => r.fulfill({ json: { current: null, exists: false, recent: [], suggested: '/Users/greg/Pictures/Argus/library' } }));
  await p.goto(`${base}?library=live`);
  await p.getByTestId('first-run').waitFor();
  await p.screenshot({ path: `${out}11-first-run.png` });
  results.push('11-first-run');
  await ctx.close();
}

// Smoke checks
async function check(name, fn) {
  if (filters.length && !filters.some((f) => name.includes(f))) return;
  try {
    await fn();
    results.push(`ok  ${name}`);
  } catch (e) {
    errors.push(`FAIL ${name}: ${e.message}`);
  }
}

const verdict = (p, i) => p.locator('.tile').nth(i).getAttribute('data-verdict');

await check('smoke-rename-shoot', async () => {
  const { p, ctx } = await page('#/cull');
  await p.getByTestId('shoot-tahoe').dblclick();
  await p.keyboard.type('Boat tour');
  await p.keyboard.press('Enter');
  await p.waitForTimeout(200);
  const name = await p.getByTestId('shoot-tahoe').textContent();
  if (!name.includes('2026-10-12_Boat-tour')) throw new Error(`sidebar shows ${name}`);
  const head = await p.locator('h1').first().textContent();
  if (head !== '2026-10-12_Boat-tour') throw new Error(`header shows ${head}`);
  // Esc cancels.
  await p.getByTestId('shoot-tahoe').focus();
  await p.keyboard.press('Enter');
  await p.keyboard.type('Nope');
  await p.keyboard.press('Escape');
  if (!(await p.getByTestId('shoot-tahoe').textContent()).includes('Boat-tour')) throw new Error('Esc renamed');
  await ctx.close();
});

await check('smoke-grid-size-keeps-columns-sane', async () => {
  const { p, ctx } = await page('#/cull');
  const cols = () => p.locator('.grid-row').first().locator('.tile').count();
  const before = await cols();
  await p.locator('.tile').first().click();
  await p.keyboard.press('Equal');
  await p.keyboard.press('Equal');
  const bigger = await cols();
  await p.mouse.move(700, 400);
  await p.keyboard.down('Control');
  for (let i = 0; i < 8; i++) await p.mouse.wheel(0, 60);
  await p.keyboard.up('Control');
  await p.waitForTimeout(200);
  const smaller = await cols();
  if (!(bigger < before && smaller > bigger)) throw new Error(`columns ${before} -> ${bigger} -> ${smaller}`);
  await ctx.close();
});

await check('smoke-flag-keys-advance', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().click();
  await p.keyboard.press('x');
  if ((await verdict(p, 0)) !== 'reject') throw new Error(`X: expected reject, got ${await verdict(p, 0)}`);
  // Flagging jumped to the next Photo, so P lands on tile 1.
  await p.keyboard.press('p');
  if ((await verdict(p, 1)) !== 'pick') throw new Error(`P after X: expected tile 1 pick, got ${await verdict(p, 1)}`);
  await p.keyboard.press('ArrowLeft');
  await p.keyboard.press('ArrowLeft');
  await p.keyboard.press('u');
  if ((await verdict(p, 0)) !== 'unflagged') throw new Error(`U: expected unflagged, got ${await verdict(p, 0)}`);
  await ctx.close();
});

// Under the Unflagged filter a flagged Photo leaves the grid, and sorted by stars a rated one
// moves: either way the cursor goes to the Photo that was next, never one past it.
await check('smoke-flag-under-filter-and-sort-goes-to-next', async () => {
  const { p, ctx } = await page('#/cull');
  const ids = () => p.locator('.grid-v .tile').evaluateAll((els) => els.map((e) => e.dataset.testid));
  const cur = () => p.locator('.grid-v .tile.cur').getAttribute('data-testid');
  await p.locator('.filter-seg [title="Unflagged"]').click();
  await p.locator('.grid-v .tile').first().click();
  const before = await ids();
  await p.keyboard.press('p');
  if ((await cur()) !== before[1]) throw new Error(`P under Unflagged: cursor on ${await cur()}, expected ${before[1]}`);
  await p.keyboard.press('x');
  if ((await cur()) !== before[2]) throw new Error(`X after P under Unflagged: cursor on ${await cur()}, expected ${before[2]}`);
  await p.locator('.filter-seg [title="All"]').click();
  await p.keyboard.press('Shift+S');
  await p.locator('.grid-v .tile').nth(3).click();
  const sorted = await ids();
  await p.keyboard.press('5');
  if ((await cur()) !== sorted[4]) throw new Error(`5 sorted by stars: cursor on ${await cur()}, expected ${sorted[4]}`);
  await ctx.close();
});

// Esc right after a flag in the Loupe stays on the grid: the Loupe's move to the next Photo
// must not fire once it's gone. Fast keys flag one Photo each.
await check('smoke-loupe-flag-then-esc-and-fast-keys', async () => {
  const { p, ctx } = await page('#/cull');
  const first = await p.locator('.grid-v .tile').first().getAttribute('data-testid');
  const ids = await p.locator('.grid-v .tile').evaluateAll((els) => els.slice(0, 2).map((e) => e.dataset.testid.replace('tile-', '')));
  await p.locator('.grid-v .tile').first().dblclick();
  await p.waitForURL(/#\/loupe\//);
  await p.keyboard.press('p');
  await p.keyboard.press('Escape');
  await p.waitForTimeout(400);
  if (!p.url().includes('#/cull')) throw new Error(`Esc after P ended on ${p.url().split('#')[1]}`);
  if ((await p.locator(`[data-testid="${first}"]`).getAttribute('data-verdict')) !== 'pick') throw new Error('P in the Loupe did not pick');
  await p.goto(`${base}#/loupe/${ids[1]}`);
  await p.waitForTimeout(500);
  const strip = p.getByTestId('filmstrip').locator('button');
  const names = await strip.evaluateAll((els) => {
    const i = els.findIndex((e) => e.classList.contains('on'));
    return els.slice(i, i + 3).map((e) => e.getAttribute('aria-label'));
  });
  await p.keyboard.press('x');
  await p.keyboard.press('x');
  await p.keyboard.press('x');
  await p.waitForTimeout(500);
  const v = await Promise.all(names.map(async (n) => ((await strip.filter({ has: p.locator(`xpath=self::*[@aria-label="${n}"]`) }).first().getAttribute('class')) ?? '').includes('rejected') ? 'reject' : 'not'));
  if (v.join() !== 'reject,reject,reject') throw new Error(`three fast X in the Loupe flagged ${v}`);
  await ctx.close();
});

const stars = (p, i) => p.locator('.tile').nth(i).locator('.stars').first();

await check('smoke-star-keys-confirm-and-advance', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().click();
  const ai = await stars(p, 0).getAttribute('data-stars');
  if (!ai || ai === '0' || (await stars(p, 0).getAttribute('data-you'))) throw new Error(`tile 0 should show suggested stars, got ${ai}`);
  // Pressing the AI's own number confirms it: same stars, now the person's.
  await p.keyboard.press(ai);
  if ((await stars(p, 0).getAttribute('data-you')) == null || (await stars(p, 0).getAttribute('data-stars')) !== ai) throw new Error('confirming kept or lost the wrong stars');
  // Rating moved on, so 5 lands on tile 1; 0 there clears it.
  await p.keyboard.press('5');
  if ((await stars(p, 1).getAttribute('data-stars')) !== '5') throw new Error('5 did not rate tile 1');
  await p.keyboard.press('ArrowLeft');
  await p.keyboard.press('0');
  if ((await stars(p, 1).getAttribute('data-stars')) !== '0') throw new Error('0 did not clear tile 1');
  await ctx.close();
});

await check('smoke-best-photos-and-star-filter', async () => {
  const { p, ctx } = await page('#/cull');
  await p.keyboard.press('b');
  const best = p.getByTestId('best-photos').locator('.tile');
  const n = await best.count();
  if (n < 5 || n > 20) throw new Error(`expected about one in ten Best Photos, got ${n}`);
  if ((await best.first().locator('.rank').textContent()) !== '#1') throw new Error('Best Photos should open on #1');
  await p.keyboard.press('b');
  await p.getByTestId('stars-filter').click();
  await p.getByRole('menuitem', { name: /and up/ }).first().click(); // 4 and up
  const shown = await p.locator('.grid-v .tile .stars').evaluateAll((els) => els.map((e) => Number(e.getAttribute('data-stars'))));
  if (!shown.length || shown.some((x) => x < 4)) throw new Error(`4+ filter showed ${shown}`);
  await ctx.close();
});

await check('smoke-sort-by-stars', async () => {
  const { p, ctx } = await page('#/cull');
  const stars = () => p.locator('.grid-v .tile .stars').evaluateAll((els) => els.map((e) => Number(e.getAttribute('data-stars'))));
  const before = await stars();
  await p.keyboard.press('Shift+S');
  const sorted = await stars();
  if (sorted.some((x, i) => i > 0 && x > sorted[i - 1])) throw new Error(`⇧S did not sort by stars, most first: ${sorted}`);
  if (sorted.join() === before.join()) throw new Error('⇧S changed nothing');
  await p.reload();
  await p.getByTestId('sort').waitFor();
  if ((await p.getByTestId('sort').getAttribute('data-sort')) !== 'stars:desc') throw new Error('the sort was not remembered');
  await p.getByTestId('sort').click();
  await p.getByTestId('sort-asc').click();
  const asc = await stars();
  if (asc.some((x, i) => i > 0 && x < asc[i - 1])) throw new Error(`Fewest first is not ascending: ${asc}`);
  await p.getByTestId('sort').click();
  await p.getByTestId('sort-capture').click();
  if ((await stars()).join() !== before.join()) throw new Error('Capture time did not restore the grid');
  await ctx.close();
});

await check('smoke-flag-click-toggle', async () => {
  const { p, ctx } = await page('#/cull');
  const tile = p.locator('.tile').nth(3);
  await tile.hover();
  await tile.getByRole('button', { name: 'Pick' }).click();
  if ((await tile.getAttribute('data-verdict')) !== 'pick') throw new Error('click Pick did not pick');
  await tile.getByRole('button', { name: 'Pick' }).click();
  if ((await tile.getAttribute('data-verdict')) !== 'unflagged') throw new Error('second click did not clear');
  await ctx.close();
});

await check('smoke-multiselect-flag', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').nth(0).click();
  await p.locator('.tile').nth(3).click({ modifiers: ['Shift'] });
  await p.keyboard.press('x');
  for (let i = 0; i <= 3; i++) if ((await verdict(p, i)) !== 'reject') throw new Error(`tile ${i} not rejected`);
  await p.keyboard.press('Control+z');
  await p.waitForTimeout(100);
  if ((await verdict(p, 0)) === 'reject' && (await verdict(p, 3)) === 'reject') throw new Error('undo did not revert the batch');
  await ctx.close();
});

await check('smoke-compare-keep-best', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile:has(.badge)').first().click();
  await p.keyboard.press('c');
  await p.waitForTimeout(400);
  const cells = p.locator('.stack-row [data-testid^="survey-"]');
  const n = await cells.count();
  if (n < 2) throw new Error(`expected a Stack, got ${n}`);
  await p.keyboard.press('k');
  const vs = await cells.evaluateAll((els) => els.map((e) => e.getAttribute('data-verdict')));
  if (vs.filter((v) => v === 'pick').length !== 1 || vs.filter((v) => v === 'reject').length !== n - 1) throw new Error(`got ${vs}`);
  await ctx.close();
});

await check('smoke-stack-toggle', async () => {
  const { p, ctx } = await page('#/cull');
  const cover = p.locator('.tile.stk-shut').first();
  const id = await cover.getAttribute('data-testid');
  const size = Number((await cover.locator('[data-testid=stack-badge]').textContent()).trim());
  // S expands the current Stack: its frames number themselves and sit on a band.
  await cover.click();
  await p.keyboard.press('s');
  await p.waitForTimeout(200);
  const labels = await p.locator('.tile.stk-open [data-testid=stack-badge]').allTextContents();
  if (labels.length !== size || labels[0] !== `1/${size}`) throw new Error(`frame badges ${labels}`);
  if (!(await p.locator('[data-testid=stack-band]').count())) throw new Error('no band behind the expanded Stack');
  // S from another frame collapses and lands back on the cover.
  await p.keyboard.press('ArrowRight');
  await p.keyboard.press('s');
  await p.waitForTimeout(200);
  if (await p.locator('.tile.stk-open').count()) throw new Error('S did not collapse');
  if ((await p.locator('.tile.cur').getAttribute('data-testid')) !== id) throw new Error('collapse did not land on the cover');
  // The band collapses on click too.
  await p.locator('.tile.stk-shut [data-testid=stack-badge]').first().click();
  await p.waitForTimeout(200);
  await p.locator('[data-testid=stack-band]').first().click({ position: { x: 2, y: 40 } });
  await p.waitForTimeout(200);
  if (await p.locator('.tile.stk-open').count()) throw new Error('clicking the band did not collapse');
  await ctx.close();
});

await check('smoke-bursts-keep-and-next', async () => {
  const { p, ctx } = await page('#/cull');
  await p.keyboard.press('n');
  await p.waitForTimeout(400);
  const first = await p.locator('.view-file').textContent();
  const cells = p.locator('.burst-cell');
  const n = await cells.count();
  if (n < 2) throw new Error(`expected a Stack, got ${n} frames`);
  const ids = await cells.evaluateAll((els) => els.map((e) => e.getAttribute('data-testid')));
  // Enter keeps the focused (AI best) frame and moves on; the rest stay Unflagged by default.
  await p.keyboard.press('Enter');
  await p.waitForTimeout(200);
  if ((await p.locator('.view-file').textContent()) === first) throw new Error('Enter did not move to the next Stack');
  await p.keyboard.press('ArrowUp');
  await p.waitForTimeout(200);
  const vs = await Promise.all(ids.map((id) => p.getByTestId(id).getAttribute('data-verdict')));
  if (vs[0] !== 'pick' || vs.slice(1).some((v) => v === 'pick')) throw new Error(`expected only the best frame kept, got ${vs}`);
  if ((await p.getByTestId('bursts-progress').getAttribute('title'))?.startsWith('0 ')) throw new Error('progress did not move');
  // Rejected as the Rest: two Picks of your own, Enter rejects the others.
  await p.getByRole('radio', { name: /Reject the frames/ }).click();
  await p.keyboard.press('ArrowDown');
  await p.waitForTimeout(200);
  const ids2 = await p.locator('.burst-cell').evaluateAll((els) => els.map((e) => e.getAttribute('data-testid')));
  await p.keyboard.press('p');
  await p.keyboard.press('ArrowRight');
  await p.keyboard.press('p');
  await p.keyboard.press('Enter');
  await p.waitForTimeout(150);
  await p.keyboard.press('ArrowUp');
  await p.waitForTimeout(200);
  const vs2 = await Promise.all(ids2.map((id) => p.getByTestId(id).getAttribute('data-verdict')));
  if (vs2.filter((v) => v === 'pick').length !== 2 || vs2.filter((v) => v === 'reject').length !== ids2.length - 2) throw new Error(`got ${vs2}`);
  await ctx.close();
});

await check('smoke-look-slider', async () => {
  const { p, ctx } = await page('#/look');
  await p.getByRole('button', { name: /Warm, soft/ }).click();
  await p.waitForTimeout(2600);
  const before = await p.locator('.look-after img').first().evaluate((el) => getComputedStyle(el).filter);
  await p.getByRole('button', { name: 'Adjust' }).click();
  await p.getByRole('slider', { name: 'Exposure' }).first().focus();
  for (let i = 0; i < 10; i++) await p.keyboard.press('ArrowRight');
  const after = await p.locator('.look-after img').first().evaluate((el) => getComputedStyle(el).filter);
  if (before === after) throw new Error('slider did not change render');
  await ctx.close();
});

await check('smoke-keepers-first-holds-what-you-passed', async () => {
  const { p, ctx } = await page('#/cull');
  await p.keyboard.press('Shift+K');
  if ((await p.getByTestId('sort').getAttribute('data-sort')) !== 'keepers:asc') throw new Error('⇧K did not choose Keepers first');
  await p.keyboard.press('Enter');
  await p.waitForTimeout(400);
  const seen = [await p.evaluate(() => location.hash)];
  for (let i = 0; i < 12; i++) {
    await p.keyboard.press(i % 3 ? 'p' : 'x');
    await p.waitForTimeout(240);
    seen.push(await p.evaluate(() => location.hash));
  }
  await p.waitForTimeout(1600);
  if (!(await p.getByTestId('up-next').count())) throw new Error('no Up next mark in the filmstrip');
  // Walking back retraces the frames flagged, in order: nothing behind moved.
  for (let i = seen.length - 2; i >= 0; i--) {
    await p.keyboard.press('ArrowLeft');
    await p.waitForTimeout(80);
    const at = await p.evaluate(() => location.hash);
    if (at !== seen[i]) throw new Error(`frame ${i} moved: ${at} vs ${seen[i]}`);
  }
  await ctx.close();
});

await check('smoke-pre-edit-accept-in-loupe', async () => {
  const { p, ctx } = await page('#/edit/p15');
  await p.getByRole('slider', { name: 'Temp' }).first().focus();
  for (let i = 0; i < 5; i++) await p.keyboard.press('ArrowRight');
  await p.goto(`${base}#/cull`);
  await p.waitForTimeout(400);
  const line = p.getByTestId('pre-edits');
  const n = Number(/ready for (\d+)/.exec((await line.textContent()) ?? '')?.[1]);
  if (!n) throw new Error('no Shoot match line after a hand edit');
  await p.getByTestId('pre-edits-show').click();
  await p.keyboard.press('Enter');
  await p.waitForTimeout(300);
  if (!(await p.getByTestId('loupe-match').count())) throw new Error('the Loupe shows no Shoot match');
  await p.keyboard.press('a');
  await p.waitForTimeout(300);
  if (await p.getByTestId('loupe-match').count()) throw new Error('A did not accept the Shoot match');
  await p.keyboard.press('g');
  await p.waitForTimeout(300);
  const left = Number(/ready for (\d+)/.exec((await p.getByTestId('pre-edits').textContent()) ?? '')?.[1] ?? 0);
  if (left !== n - 1) throw new Error(`expected ${n - 1} left, got ${left}`);
  await ctx.close();
});

await check('smoke-sync-to-look', async () => {
  const { p, ctx } = await page('#/edit/p15');
  await p.getByRole('slider', { name: 'Temp' }).first().focus();
  for (let i = 0; i < 5; i++) await p.keyboard.press('ArrowRight');
  await p.getByRole('button', { name: 'Sync to Look' }).click();
  await p.goto(`${base}#/look`);
  await p.waitForTimeout(400);
  const h = await p.locator('[data-testid="look-card"] h3').getAttribute('aria-label');
  if (!/Shoot Look/.test(h ?? '')) throw new Error(`expected a Shoot Look, got ${h}`);
  await ctx.close();
});

await check('smoke-agent-edit-masks', async () => {
  const { p, ctx } = await page('#/edit/p15');
  await p.getByPlaceholder(/A bit more on the face/).click();
  await p.getByRole('button', { name: /Brighten her face/ }).click();
  await p.waitForTimeout(3200);
  await p.getByRole('tab', { name: 'Masking' }).click();
  const n = await p.locator('.mask-row').count();
  if (n < 2) throw new Error(`expected Masks, got ${n}`);
  await ctx.close();
});

// Applies a built-in Preset from the Presets tab (its group opened if closed), then back to Edit.
async function applyPresetTab(p, id, group) {
  await p.getByRole('tab', { name: 'Presets' }).click();
  const sec = p.getByTestId(`section-presets-${group}`);
  if ((await sec.getAttribute('class')).split(' ').indexOf('open') < 0) await sec.locator('.dev-section-toggle').click();
  await sec.getByTestId(`preset-${id}`).click();
  await p.getByRole('tab', { name: 'Edit', exact: true }).click();
}

await check('smoke-edit-v2-recommended-fixes-people-crop', async () => {
  const { p, ctx } = await page('#/edit/p15');
  // A Look from Presets (Edit's Recommended row moved there, Greg 2026-10-22), then a fix that toggles.
  const temp = async () => Number((await p.locator('.slider', { hasText: 'Temp' }).first().locator('.val').textContent()).replace('−', '-'));
  const t0 = await temp();
  await applyPresetTab(p, 'warm-film', 'Film');
  if ((await temp()) !== t0 + 12) throw new Error(`Warm film should add 12 to temp ${t0}, got ${await temp()}`);
  await applyPresetTab(p, 'natural', 'Essentials');
  if ((await temp()) !== t0) throw new Error(`switching Looks left temp ${await temp()}, was ${t0}`);
  await p.getByTestId('fix-shadows').click();
  if ((await p.getByTestId('fix-shadows').getAttribute('aria-pressed')) !== 'true') throw new Error('Lift shadows did not turn on');
  await p.getByTestId('fix-shadows').click();
  if ((await p.getByTestId('fix-shadows').getAttribute('aria-pressed')) !== 'false') throw new Error('Lift shadows did not turn off');
  // A person Mask with the people picker and parts.
  await p.getByRole('tab', { name: 'Masking' }).click();
  await p.getByTestId('mask-create').getByRole('button', { name: 'People' }).click();
  await p.getByRole('button', { name: 'Person 2' }).click();
  await p.getByLabel('Hair', { exact: true }).click();
  if (!(await p.locator('.mask-row.sel', { hasText: 'Faces · Person 2' }).count())) throw new Error('the person Mask did not follow the picker');
  // Crop at 4:5 and back.
  await p.getByRole('tab', { name: 'Crop' }).click();
  await p.getByRole('radio', { name: '4:5' }).click();
  if ((await p.getByRole('radio', { name: '4:5' }).getAttribute('aria-checked')) !== 'true') throw new Error('4:5 did not take');
  if (!(await p.getByTestId('crop-overlay').count())) throw new Error('no crop box on the photo');
  await ctx.close();
});

await check('smoke-lens-corrections', async () => {
  const { p, ctx } = await page('#/edit/p15');
  const optics = p.getByTestId('section-optics');
  await optics.locator('.dev-section-toggle').click();
  const seg = optics.getByRole('group', { name: 'Lens corrections' });
  if ((await seg.getByRole('radio', { name: 'What the camera corrected in its own JPEG' }).getAttribute('data-state')) !== 'on') throw new Error('Lens corrections should start As camera');
  await seg.getByText('Off', { exact: true }).click();
  await p.waitForTimeout(150);
  await optics.scrollIntoViewIfNeeded();
  await p.screenshot({ path: `${out}smoke-lens-corrections.png` });
  // ⌘Z puts it back.
  await p.keyboard.press('Control+z');
  await p.waitForTimeout(150);
  if ((await seg.getByRole('radio', { name: 'What the camera corrected in its own JPEG' }).getAttribute('data-state')) !== 'on') throw new Error('Undo should put Lens corrections back to As camera');
  await ctx.close();
});

await check('smoke-copy-paste-settings', async () => {
  const { p, ctx } = await page('#/edit/p15');
  const temp = async () => Number((await p.locator('.slider', { hasText: 'Temp' }).first().locator('.val').textContent()).replace('−', '-'));
  await applyPresetTab(p, 'warm-film', 'Film');
  // ⇧⌘C asks which groups, Lightroom-style; untick White balance and nothing of it travels.
  await p.keyboard.press('Control+Shift+c');
  await p.getByTestId('settings-dialog').waitFor();
  await p.screenshot({ path: `${out}smoke-copy-settings-dialog.png` });
  await p.getByTestId('paste-wb').click();
  await p.keyboard.press('Enter');
  await p.keyboard.press('Control+ArrowRight');
  await p.waitForTimeout(300);
  await p.keyboard.press('Control+Shift+v');
  await p.waitForTimeout(200);
  if ((await temp()) !== 0) throw new Error(`White balance was unticked but temp is ${await temp()}`);
  // ⌥⌘V pastes the previous Photo's settings with the groups last chosen; tick wb again first.
  await p.keyboard.press('Control+Shift+c');
  await p.getByTestId('paste-wb').click();
  await p.keyboard.press('Escape');
  await p.keyboard.press('Control+Alt+v');
  await p.waitForTimeout(200);
  if ((await temp()) !== 0) throw new Error('Esc should not have changed the remembered groups');
  // The dialog remembers the last groups, Lightroom-style: tick White balance and copy.
  await p.keyboard.press('Control+ArrowLeft');
  await p.keyboard.press('Control+Shift+c');
  if ((await p.getByTestId('paste-wb').getAttribute('data-state')) !== 'unchecked') throw new Error('the dialog forgot the last groups');
  await p.getByTestId('paste-wb').click();
  await p.keyboard.press('Enter');
  await p.keyboard.press('Control+ArrowRight');
  await p.waitForTimeout(300);
  await p.keyboard.press('Control+Shift+v');
  await p.waitForTimeout(200);
  if ((await temp()) === 0) throw new Error('pasting with White balance ticked left temp at 0');
  await p.keyboard.press('Control+z');
  await p.waitForTimeout(200);
  if ((await temp()) !== 0) throw new Error(`Undo left temp at ${await temp()}`);
  await ctx.close();
});

await check('smoke-esc-on-a-menu-keeps-the-selection', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').nth(0).click();
  await p.locator('.tile').nth(2).click({ modifiers: ['Shift'] });
  const selected = () => p.locator('.tile.sel').count();
  const before = await selected();
  await p.locator('.tile').nth(0).click({ button: 'right' });
  await p.getByRole('menu').waitFor();
  await p.keyboard.press('Escape');
  await p.getByRole('menu').waitFor({ state: 'detached' });
  if ((await selected()) !== before) throw new Error(`Esc on the menu changed the selection (${before} -> ${await selected()})`);
  await p.keyboard.press('Escape');
  await p.waitForTimeout(100);
  if ((await selected()) >= before) throw new Error('a second Esc should clear the selection');
  await ctx.close();
});

await check('smoke-sync-settings-from-grid', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').nth(0).click();
  await p.locator('.tile').nth(3).click({ modifiers: ['Shift'] });
  await p.locator('.tile').nth(0).click({ button: 'right' });
  await p.getByRole('menuitem', { name: /Sync settings to 3 selected/ }).waitFor();
  await p.screenshot({ path: `${out}smoke-sync-settings-menu.png` });
  await p.getByRole('menuitem', { name: /Sync settings to 3 selected/ }).click();
  await p.getByTestId('settings-dialog').waitFor();
  await p.keyboard.press('Escape');
  await p.waitForTimeout(200);
  if (!(await p.getByText('4 selected').count())) throw new Error('Esc on the dialog also cleared the selection');
  await p.keyboard.press('Control+Shift+s');
  const title = await p.getByTestId('settings-dialog').locator('h2').textContent();
  if (!/to 3 Photos/.test(title ?? '')) throw new Error(`sync dialog says ${title}`);
  await p.keyboard.press('Escape');
  if (!p.url().includes('#/cull')) throw new Error('Esc left the grid');
  await ctx.close();
});

await check('smoke-presets-apply-and-undo', async () => {
  const { p, ctx } = await page('#/edit/p15');
  await p.getByRole('tab', { name: 'Presets' }).click();
  if ((await p.getByTestId('presets-suggested').locator('.preset').count()) < 4) throw new Error('no Suggested row');
  await p.getByTestId('section-presets-Black and white').locator('.dev-section-toggle').click();
  await p.getByTestId('preset-bw-contrast').click();
  await p.waitForTimeout(200);
  if ((await p.locator('.preset.on').count()) !== 1) throw new Error('the preset did not apply');
  const amount = p.getByTestId('preset-amount');
  if (!/B&W contrast/.test(await amount.textContent())) throw new Error('the Amount slider does not name the preset');
  await amount.getByRole('slider').focus();
  await p.keyboard.press('PageDown');
  if (Number(await amount.locator('.val').textContent()) >= 100) throw new Error('the Amount did not go down');
  if ((await p.locator('.preset.on').count()) !== 1) throw new Error('changing the Amount took the preset off');
  await p.keyboard.press('Meta+z');
  await p.keyboard.press('Control+z');
  await p.waitForTimeout(200);
  if ((await p.locator('.preset.on').count()) !== 0) throw new Error('Undo left the preset on');
  await ctx.close();
});

await check('smoke-suggestions-preview-accept-undo', async () => {
  const { p, ctx } = await page('#/edit/p15');
  const takes = await p.locator('[data-testid="take"]').count();
  if (takes < 3) throw new Error(`expected your edit plus takes, got ${takes}`);
  await p.keyboard.press('Tab');
  await p.keyboard.press('Tab');
  const flag = await p.locator('[data-testid="previewing"]').textContent();
  if (!/Punchy/.test(flag ?? '')) throw new Error(`Tab twice should preview the second take, got ${flag}`);
  if (!(await p.locator('[data-testid="take-diff"]').isVisible())) throw new Error('no diff for the take');
  await p.keyboard.press('Enter');
  await p.waitForTimeout(200);
  if (await p.locator('[data-testid="previewing"]').count()) throw new Error('still previewing after accept');
  const exposure = await p.getByRole('slider', { name: 'Exposure' }).first().getAttribute('aria-valuenow');
  // Punchy's own exposure (dummy takes in data/suggestions.ts): accepting at 100% sets it.
  if (Number(exposure) !== 0.35) throw new Error(`accepting should set exposure 0.35, got ${exposure}`);
  await p.keyboard.press('Control+z');
  await p.waitForTimeout(200);
  const back = await p.getByRole('slider', { name: 'Exposure' }).first().getAttribute('aria-valuenow');
  if (Number(back) === 0.35) throw new Error('undo did not put the edit back');
  await p.keyboard.press('Control+Shift+a');
  await p.waitForTimeout(200);
  const toast = await p.locator('[data-sonner-toast]').first().textContent();
  if (!/Matching \d+ Photos/.test(toast ?? '')) throw new Error(`no toast for apply to the rest: ${toast}`);
  await p.evaluate(() => (location.hash = '#/edit/p16'));
  await p.waitForTimeout(400);
  const matched = await p.locator('.take-name', { hasText: /^Match / }).count();
  if (!matched) throw new Error(`apply to the rest gave the next Photo no matched take: ${await p.locator('.take-name').allTextContents()} at ${await p.evaluate(() => location.hash)}`);
  await ctx.close();
});

await check('smoke-grid-accept-first-takes', async () => {
  const { p, ctx } = await page('#/cull');
  const badges = await p.locator('[data-testid="takes-badge"]').count();
  if (!badges) throw new Error('no Suggestions badges in the grid');
  // Two Picks side by side (Picks get takes first).
  await p.locator('.tile').nth(5).click();
  await p.locator('.tile').nth(6).click({ modifiers: ['Shift'] });
  await p.keyboard.press('a');
  await p.waitForTimeout(200);
  const toast = await p.locator('[data-sonner-toast]').first().textContent();
  if (!/Suggestions on 2 Photos/.test(toast ?? '')) throw new Error(`expected one batch for 2 Photos, got ${toast}`);
  await ctx.close();
});

await check('smoke-import-path-no-spaces', async () => {
  const { p, ctx } = await page('#/import');
  const path = await p.locator('#copy').inputValue();
  if (/\s/.test(path)) throw new Error(`path has spaces: ${path}`);
  await p.locator('#copy').fill('~/Pictures/My Shoot');
  const typed = await p.locator('#copy').inputValue();
  if (/\s/.test(typed)) throw new Error(`typed path kept spaces: ${typed}`);
  await ctx.close();
});

await check('smoke-queue-drag-reorders', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.sidebar [data-testid="queue-progress"]').first().click();
  await p.getByTestId('queue-popover').waitFor();
  const labels = () => p.getByTestId('queue-jobs').locator('.job-label').allTextContents();
  const before = await labels();
  const rows = p.getByTestId('queue-jobs').getByTestId('queue-job');
  await rows.nth(2).dragTo(rows.nth(0));
  await p.waitForTimeout(200);
  const after = await labels();
  if (after[0] !== before[2]) throw new Error(`drag didn't move ${before[2]} to the top: ${after.join(' / ')}`);
  await rows.nth(2).hover();
  await p.screenshot({ path: `${out}15-queue-reorder.png` });
  await p.waitForTimeout(3300); // a poll must not undo it
  if ((await labels())[0] !== before[2]) throw new Error('the next poll put the old order back');
  await ctx.close();
});

await check('smoke-meter-shows-ai-usage', async () => {
  const { p, ctx } = await page('#/cull');
  const line = p.locator('.sidebar [data-testid="meter"]');
  const text = await line.innerText();
  if (!/AI [\d.]+% · GPU [\d.]+%/.test(text)) throw new Error(`meter line reads "${text}"`);
  await p.screenshot({ path: `${out}16-meter-line.png`, clip: { x: 0, y: 900 - 150, width: 260, height: 150 } });
  await line.click();
  await p.getByTestId('meter-panel').waitFor();
  const model = await p.getByTestId('meter-local_model').innerText();
  if (!/GB/.test(model)) throw new Error(`local model row reads "${model}"`);
  await p.screenshot({ path: `${out}16-meter-panel.png` });
  await ctx.close();
});

await check('smoke-ai-marks-show-queued-and-working', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().waitFor();
  const marks = await p.getByTestId('ai-mark').evaluateAll((els) => els.map((e) => e.className));
  if (!marks.some((c) => c.includes('queued'))) throw new Error(`no queued marks: ${marks.length}`);
  await p.getByTestId('stars-filter').click();
  await p.getByTestId('ai-filter-waiting').click();
  const left = await p.locator('.grid-v .tile').count();
  const withMark = await p.locator('.grid-v .tile [data-testid="ai-mark"]').count();
  if (!left || left !== withMark) throw new Error(`Waiting filter shows ${left} tiles, ${withMark} with a mark`);
  await ctx.close();
});

await check('smoke-next-step-after-cull', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().click();
  if (await p.getByTestId('next-step').count()) throw new Error('the next-step line should hide while there are Picks');
  // The demo cull Picks; clear every flag to get the live rules cull's state (nothing Picked).
  await p.locator('.tile').first().click({ button: 'right' });
  await p.getByRole('menuitem', { name: 'Expand all Stacks' }).click();
  await p.keyboard.press('ControlOrMeta+a');
  await p.keyboard.press('u');
  await p.keyboard.press('Escape');
  await p.getByTestId('next-step').waitFor();
  await p.locator('.tile').first().click({ button: 'right' });
  await p.getByRole('menuitem', { name: 'Collapse all Stacks' }).click();
  await p.waitForTimeout(1200);
  await p.screenshot({ path: `${out}19-next-step.png` });
  const label = await p.getByTestId('next-pick-best').textContent();
  const n = Number(label?.match(/(\d+)\s*$/)?.[1]);
  await p.getByTestId('next-pick-best').click();
  await p.waitForTimeout(200);
  const picks = await p.locator('.grid-v .tile[data-verdict="pick"]').count();
  if (!n || picks < 1) throw new Error(`Pick Best Photos (${n}) made ${picks} visible Picks`);
  if (await p.getByTestId('next-step').count()) throw new Error('the line should go once something is Picked');
  await p.keyboard.press('ControlOrMeta+z');
  await p.getByTestId('next-step').waitFor();
  await ctx.close();
});

await check('smoke-tiles-compact-by-default-j-cycles', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().waitFor();
  const shown = () => p.locator('.grid-v [data-testid="takes-badge"]').evaluateAll((els) => els.filter((e) => e.offsetParent).length);
  if (await shown()) throw new Error('compact tiles (the default) should hide the Suggestions badges');
  await p.keyboard.press('j'); // photo only
  if (await p.locator('.grid-v .tile figcaption').first().isVisible()) throw new Error('photo only should drop the caption row');
  await p.keyboard.press('j'); // everything
  if (!(await shown())) throw new Error('everything should show the Suggestions badges');
  await ctx.close();
});

await check('smoke-eye-badges-on-tiles-and-loupe', async () => {
  const { p, ctx } = await page('#/cull');
  await p.locator('.tile').first().waitFor();
  await p.keyboard.press('j');
  await p.keyboard.press('j'); // compact -> photo only -> everything, so the tile shot shows its badges
  const closed = await p.locator('.grid-v [data-testid="eyes-closed"]').count();
  const soft = await p.locator('.grid-v [data-testid="soft-focus"]').count();
  if (!closed || !soft) throw new Error(`badges: ${closed} closed, ${soft} soft`);
  // Informational only: a closed-eye Photo keeps whatever flag it had.
  const tile = p.locator('.grid-v .tile:has([data-testid="eyes-closed"])').first();
  await tile.scrollIntoViewIfNeeded();
  await tile.locator('.ph').screenshot({ path: `${out}/17-eye-badge-tile.png` });
  await tile.dblclick();
  await p.getByTestId('loupe-strip').getByTestId('eyes-closed').waitFor();
  await p.screenshot({ path: `${out}/18-loupe-eye-badge.png` });
  await ctx.close();
});

await check('smoke-restart-to-update', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const p = await ctx.newPage();
  await p.goto(`${base}?update=ready#/cull`);
  await p.getByTestId('update-restart').waitFor();
  if (!(await p.getByTestId('update-restart').innerText()).includes('0.1.2')) throw new Error('the button does not name the version');
  await p.getByTestId('update-restart').hover();
  await p.waitForTimeout(700);
  await p.screenshot({ path: `${out}/15-update-ready.png` });
  await p.evaluate(() => window.dispatchEvent(new CustomEvent('argus:settings', { detail: 'updates' })));
  await p.getByTestId('settings-update-restart').waitFor();
  await p.screenshot({ path: `${out}/16-settings-updates.png` });
  await ctx.close();
  // No update ready: no button.
  const { p: q, ctx: c2 } = await page('#/cull');
  if (await q.getByTestId('update-restart').count()) throw new Error('Restart to update shows with nothing to install');
  await c2.close();
  // The launch check found a release: one quiet toast while it downloads, no button yet.
  const c3 = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const d = await c3.newPage();
  await d.goto(`${base}?update=downloading#/cull`);
  await d.getByText('Argus 0.1.2 is downloading').waitFor();
  if (await d.getByTestId('update-restart').count()) throw new Error('Restart to update shows before the download is done');
  await d.screenshot({ path: `${out}/15-update-downloading.png` });
  await c3.close();
});

await check('smoke-settings-nav-only-selected-active', async () => {
  const { p, ctx } = await page('#/cull');
  await p.evaluate(() => window.dispatchEvent(new CustomEvent('argus:settings')));
  await p.getByTestId('settings').waitFor();
  const state = () =>
    p.evaluate(() =>
      [...document.querySelectorAll('.settings-nav-item')]
        .map((b) => ({ id: b.dataset.testid, current: b.getAttribute('aria-current'), bg: getComputedStyle(b).backgroundColor, focused: document.activeElement === b })),
    );
  const transparent = (bg) => bg === 'transparent' || /rgba\(0, 0, 0, 0\)/.test(bg);
  const verify = async (want) => {
    await p.mouse.move(1, 1); // the pointer leaves the nav, so hover can't colour anything
    await p.waitForTimeout(200);
    const items = await state();
    if (items.length < 3) throw new Error(`found ${items.length} Settings sections`);
    for (const b of items) {
      const on = b.id === `settings-${want}`;
      if (on !== (b.current === 'page')) throw new Error(`${b.id} aria-current=${b.current} with ${want} open`);
      if (on === transparent(b.bg)) throw new Error(`${b.id} background ${b.bg} with ${want} open`);
    }
  };
  await verify('ai');
  await p.getByTestId('settings-library').click();
  await verify('library');
  await p.getByTestId('settings-keys').click();
  await verify('keys');
  await ctx.close();
});

// Lightroom zoom and pan (ui/zoomPan.ts). Trackpad pinch is a synthetic ctrl+wheel
// (Chromium) and gesture events (WebKit, the Mac app); the Mac thread checks a real one.
await check('smoke-zoom-pan', async () => {
  const { p, ctx } = await page('#/edit/p15');
  const sel = '[data-testid=edit-stage] .stage-fit > .photo';
  const readout = async () => (await p.getByTestId('zoom-readout').textContent()).trim();
  const rect = () => p.locator(sel).evaluate((el) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; });
  const settle = () => p.waitForTimeout(320);
  const fire = (type, init) =>
    p.locator('[data-testid=edit-stage]').evaluate((el, [type, init]) => {
      const e = type === 'wheel' ? new WheelEvent('wheel', { bubbles: true, cancelable: true, ...init }) : Object.assign(new Event(type, { bubbles: true, cancelable: true }), init);
      el.dispatchEvent(e);
    }, [type, init]);
  const fit = await rect();
  const cx = fit.x + fit.w * 0.3;
  const cy = fit.y + fit.h * 0.4;
  // Pinch around a point: the photo spot under the fingers stays put.
  for (let i = 0; i < 10; i++) await fire('wheel', { deltaY: -30, ctrlKey: true, clientX: cx, clientY: cy });
  await settle();
  let r = await rect();
  if (r.w < fit.w * 1.5) throw new Error(`pinch did not zoom (width ${fit.w} -> ${r.w})`);
  const u = (r2) => [(cx - r2.x) / r2.w, (cy - r2.y) / r2.h];
  const [u0, v0] = u(fit);
  const [u1, v1] = u(r);
  if (Math.abs(u0 - u1) > 0.01 || Math.abs(v0 - v1) > 0.01) throw new Error(`pinch drifted: ${u0.toFixed(3)},${v0.toFixed(3)} -> ${u1.toFixed(3)},${v1.toFixed(3)}`);
  if (!/%$/.test(await readout())) throw new Error(`readout says ${await readout()}`);
  // The Navigator starts closed in Edit, opens with a click on its title and stays open.
  const navTitle = p.getByTestId('navigator').locator('.nav-title');
  if ((await navTitle.getAttribute('aria-expanded')) !== 'false') throw new Error('the Navigator starts open');
  await navTitle.click();
  if (!(await p.getByTestId('nav-view').count())) throw new Error('the Navigator shows no visible area');
  await p.reload();
  await p.waitForSelector(sel);
  if ((await p.getByTestId('navigator').locator('.nav-title').getAttribute('aria-expanded')) !== 'true') throw new Error('the Navigator forgot it was open');
  for (let i = 0; i < 10; i++) await fire('wheel', { deltaY: -30, ctrlKey: true, clientX: cx, clientY: cy });
  await settle();
  r = await rect();
  // Two-finger scroll pans.
  const before = await rect();
  await fire('wheel', { deltaX: 60, deltaY: 40, clientX: cx, clientY: cy });
  await settle();
  r = await rect();
  if (Math.abs(before.x - 60 - r.x) > 1 || Math.abs(before.y - 40 - r.y) > 1) throw new Error(`scroll panned ${before.x - r.x},${before.y - r.y}, wanted 60,40`);
  // WebKit's pinch.
  await p.keyboard.press('z');
  await settle();
  await fire('gesturestart', { scale: 1, clientX: cx, clientY: cy });
  await fire('gesturechange', { scale: 3, clientX: cx, clientY: cy });
  await fire('gestureend', { scale: 3, clientX: cx, clientY: cy });
  await settle();
  r = await rect();
  if (Math.abs(r.w / fit.w - 3) > 0.05) throw new Error(`gesture scale 3 zoomed ${r.w / fit.w}`);
  // Z toggles Fit and 100%; ⌘0 fits, ⌘+ steps in.
  await p.keyboard.press('z');
  await settle();
  if ((await readout()) !== 'Fit') throw new Error(`Z from zoomed left ${await readout()}`);
  await p.keyboard.press('z');
  await settle();
  if ((await readout()) !== '100%') throw new Error(`Z from Fit went to ${await readout()}`);
  await p.keyboard.press('ControlOrMeta+0');
  await settle();
  if ((await readout()) !== 'Fit') throw new Error(`⌘0 left ${await readout()}`);
  await p.keyboard.press('ControlOrMeta+Equal');
  await settle();
  if ((await readout()) === 'Fit') throw new Error('⌘+ did not zoom the photo');
  const appZoom = await p.evaluate(() => document.documentElement.style.zoom);
  if (appZoom) throw new Error(`⌘+ zoomed the app (${appZoom}) instead of the photo`);
  await p.keyboard.press('ControlOrMeta+0');
  await settle();
  // Click zooms to 100% at the point, click again fits.
  await p.mouse.click(cx, cy);
  await settle();
  if ((await readout()) !== '100%') throw new Error(`click went to ${await readout()}`);
  // Space-drag is the hand tool.
  r = await rect();
  await p.keyboard.down('Space');
  await p.mouse.move(cx, cy);
  await p.mouse.down();
  await p.mouse.move(cx - 80, cy - 30, { steps: 4 });
  await p.waitForTimeout(100);
  await p.mouse.up();
  await p.keyboard.up('Space');
  await settle();
  const dragged = await rect();
  if (Math.abs(r.x - dragged.x) < 40) throw new Error(`Space-drag moved the photo ${r.x - dragged.x}px`);
  if ((await readout()) !== '100%') throw new Error('Space-drag toggled the zoom');
  await p.waitForTimeout(450);
  await p.mouse.click(cx, cy);
  await settle();
  if ((await readout()) !== 'Fit') throw new Error(`second click left ${await readout()}`);
  // Navigator presets.
  await p.getByTestId('zoom-200').click();
  await settle();
  if ((await readout()) !== '200%') throw new Error(`200% preset shows ${await readout()}`);
  await p.screenshot({ path: `${out}14-edit-zoom-200.png` });
  // The crop box still drags while zoomed, and a click there doesn't toggle the zoom.
  await p.getByRole('tab', { name: 'Crop' }).click();
  await p.getByRole('radio', { name: '4:5' }).click();
  await p.getByTestId('zoom-fit').click();
  await settle();
  await p.keyboard.press('z');
  await settle();
  const shade = p.locator('.crop-shade');
  const s0 = await shade.boundingBox();
  const vis = await p.getByTestId('edit-stage').boundingBox();
  const sx = Math.max(s0.x, vis.x) + 30;
  const sy = Math.max(s0.y, vis.y) + 30;
  await p.mouse.move(sx, sy);
  await p.mouse.down();
  await p.mouse.move(sx + 60, sy, { steps: 4 });
  await p.mouse.up();
  await settle();
  const s1 = await shade.boundingBox();
  if (Math.abs(s1.x - s0.x - 60) > 3) throw new Error(`crop moved ${s1.x - s0.x}px while zoomed, wanted 60`);
  if ((await readout()) !== '100%') throw new Error(`dragging the crop changed the zoom to ${await readout()}`);
  await ctx.close();
  // The Loupe shares it.
  const l = await page('#/loupe/p15');
  const lr = () => l.p.locator('[data-testid=loupe] .stage-fit > .photo').evaluate((el) => el.getBoundingClientRect().width);
  const w0 = await lr();
  await l.p.locator('[data-testid=loupe]').evaluate((el) => {
    const b = el.getBoundingClientRect();
    for (let i = 0; i < 8; i++) el.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -30, ctrlKey: true, clientX: b.left + b.width / 2, clientY: b.top + b.height / 2 }));
  });
  await l.p.waitForTimeout(320);
  if ((await lr()) < w0 * 1.5) throw new Error('pinch did not zoom the Loupe');
  await l.p.screenshot({ path: `${out}14b-loupe-zoomed.png` });
  await l.p.keyboard.press('ArrowRight');
  await l.p.waitForTimeout(500);
  if ((await lr()) < w0 * 1.5) throw new Error('the zoom did not carry to the next photo');
  await l.ctx.close();
});

await browser.close();
await server.close();
console.log(results.join('\n'));
// What this run shot, so scripts/shot-diff.mjs never compares a stale PNG from an earlier run.
writeFileSync(`${out}last-run.json`, JSON.stringify({ filters, shots: results.filter((r) => !r.startsWith('ok  ')) }, null, 1) + '\n');
console.log(`\nShots in ${out}`);
if (errors.length) {
  console.error('\nProblems:\n' + errors.join('\n'));
  process.exit(1);
}
