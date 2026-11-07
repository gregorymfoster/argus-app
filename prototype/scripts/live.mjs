// Live smoke test: the prototype against a real Library through `argus serve`.
// Works on a temporary copy of the Library, so the original is never changed.
// Starts the bridge and Vite, opens ?library=live, checks every Photo shows its
// engine preview, flags one with X or P and confirms the Verdict reached the Library
// (and that ⌘Z, ⇧⌘Z and star keys are engine writes too, and View › Zoom and pinch zoom the
// app in the grid and the photo in Loupe),
// then asks the Agent to cull against a mock Claude endpoint and checks the AI
// Verdicts and Stacks land. Then it opens a Photo in Edit, asks the Agent for "warmer,
// less contrast" (the mock answers with an edit_recipe call), checks the new Recipe
// Version and its History row, and undoes it from the chat. Between the two it exports
// the Picks through the Export dialog and checks the files. Last it adds a Sky Mask and checks the
// engine renders it and the overlay shows the engine's own Mask. Screenshots land in
// out/shots/live-*.png.
//
//   node scripts/live.mjs <library-dir>     (build one with `argus import <folder> --library <dir>`)
//   node scripts/live.mjs <lib> --only zoom,flags   just those steps (or LIVE_ONLY=zoom,flags)
//   ARGUS_BIN=../target/fast/argus          to pick the binary (default shown)
//   PW_BROWSER=webkit                       WebKit instead of Chromium

import { createServer } from 'vite';
import { chromium, webkit } from 'playwright';
import { spawn } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, mkdirSync, readdirSync } from 'node:fs';
import { createServer as httpServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const source = process.argv[2];
if (!source) {
  console.error('usage: node scripts/live.mjs <library-dir>');
  process.exit(2);
}
// --only flags,zoom runs just those steps (each one navigates for itself), for a fast loop.
const STEPS = ['grid', 'flags', 'zoom', 'cancel', 'cull', 'auto', 'export', 'agent', 'agent-batch', 'sliders', 'suggest', 'masks', 'look', 'rename', 'remove', 'views', 'keywords', 'search', 'backup'];
const onlyArg = process.argv.indexOf('--only');
const only = onlyArg > 0 ? process.argv[onlyArg + 1]?.split(',').filter(Boolean) ?? [] : (process.env.LIVE_ONLY?.split(',').filter(Boolean) ?? []);
const unknown = only.filter((s) => !STEPS.includes(s));
if (unknown.length) {
  console.error(`live: unknown step(s) ${unknown.join(', ')}; steps are ${STEPS.join(', ')}`);
  process.exit(2);
}
const run = (step) => !only.length || only.includes(step);
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const out = new URL('../out/shots/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });

const library = join(mkdtempSync(join(tmpdir(), 'argus-live-')), 'lib');
cpSync(source, library, { recursive: true });

// A stand-in for the model (Ollama's /api/chat, or the Messages API): picks photo 1,
// stacks 1 and 2, rejects the rest.
let mockCalls = 0;
let agentCalls = 0;
let suggestCalls = 0;
// Photos an Agent turn asked to "warm every photo" edits all at once (the agent-batch step).
let batchIds = [];
const mock = httpServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    // Ollama's /api/tags and /api/ps (is it running, which models): one model, loaded.
    if (!body) return res.end(JSON.stringify({ models: [{ name: 'mock', size: 1 }] }));
    const req = JSON.parse(body);
    res.setHeader('content-type', 'application/json');
    // Ollama model load (Argus loads before it culls): no messages, nothing to answer.
    if (req.messages.length === 0) return res.end(JSON.stringify({ message: { role: 'assistant', content: '' }, done: true }));
    // An Agent turn (step schema): edit the Photo in focus, then say what changed.
    if (req.format?.required?.includes('calls')) {
      agentCalls++;
      const focus = Number(/In focus: Photo (\d+)/.exec(req.messages[1].content)?.[1]);
      const done = req.messages[req.messages.length - 1].content.includes('edit_recipe ->');
      const every = req.messages.some((m) => typeof m.content === 'string' && m.content.includes('warm every photo'));
      if (every) {
        const step = done ? { say: 'Mock batch: warmed every Photo.', calls: [] } : { say: 'Warming all', calls: [{ tool: 'edit_recipe', args: { photo_ids: batchIds, set: { temp: 9 }, relative: true } }] };
        return res.end(JSON.stringify({ message: { content: JSON.stringify(step) }, done: true }));
      }
      const step = done
        ? { say: 'Mock edit: Temperature +12, Contrast -15.', calls: [] }
        : { say: 'Warming', calls: [{ tool: 'edit_recipe', args: { photo_ids: [focus], set: { temp: 12, contrast: -15 }, relative: true } }] };
      return res.end(JSON.stringify({ message: { content: JSON.stringify(step) }, done: true }));
    }
    // Suggestions for one Photo (the background Queue): three takes, one with a crop.
    if (req.format?.required?.includes('suggestions')) {
      suggestCalls++;
      const takes = { suggestions: [
        { title: 'Mock natural', why: 'Lifts the shadows.', set: { exposure: 0.3, shadows: 25 } },
        { title: 'Mock airy', why: 'Brighter, cropped in.', set: { exposure: 0.7, 'crop.w': 0.8, 'crop.h': 0.8, 'crop.x': 0.1, 'crop.y': 0.1 } },
        { title: 'Mock mono', why: 'Black and white.', set: { saturation: -100, contrast: 25 } },
      ] };
      return res.end(JSON.stringify({ message: { content: JSON.stringify(takes) }, done: true }));
    }
    mockCalls++;
    const ollama = !Array.isArray(req.messages[0].content);
    const n = ollama ? req.messages.filter((m) => m.images).length : req.messages[0].content.filter((b) => b.type === 'image').length;
    const photos = Array.from({ length: n }, (_, i) => ({
      n: i + 1,
      verdict: i === 0 ? 'pick' : 'reject',
      reason: i === 0 ? 'best of 2' : 'weaker frame',
      stack: i < 2 ? 1 : null,
      rank: i < 2 ? i + 1 : null,
    }));
    const text = JSON.stringify({ summary: 'Mock cull.', photos });
    // Ollama culls stream: think, then send each Photo's answer a beat apart, so the
    // cull's progress bar has something to show.
    if (ollama && req.stream) {
      res.setHeader('content-type', 'application/x-ndjson');
      const line = (v) => res.write(JSON.stringify(v) + '\n');
      const pieces = text.split(/(?=\{"n":)/);
      line({ message: { content: '', thinking: 'Looking' }, done: false });
      let i = 0;
      const next = () => {
        if (i < pieces.length) {
          line({ message: { content: pieces[i++] }, done: false });
          return void setTimeout(next, 700);
        }
        line({ message: { content: '' }, done: true });
        res.end();
      };
      return void setTimeout(next, 700);
    }
    res.end(JSON.stringify(ollama ? { message: { content: text }, done: true } : { stop_reason: 'end_turn', content: [{ type: 'text', text }] }));
  });
}).listen(7879, '127.0.0.1');

// The mock model needs no turn at the real one: a private model lock and Foreground markers,
// so a cull Greg's app is running neither stalls this check nor holds its Queue (2026-10-20).
const modelLockDir = mkdtempSync(join(tmpdir(), 'argus-model-'));
const foregroundDir = mkdtempSync(join(tmpdir(), 'argus-foreground-'));
const bridge = spawn(bin, ['serve', '--library', library, '--port', '7878'], {
  stdio: ['ignore', 'inherit', 'inherit'],
  // Full: never throttle (pause) a real Ollama on this machine during a smoke test.
  env: { ...process.env, ARGUS_PROVIDER: 'ollama', ARGUS_CULL_CHECK: 'model', ARGUS_CULL_ASK: 'all', OLLAMA_HOST: '127.0.0.1:7879', ARGUS_COMPUTE: 'full', ARGUS_REFINE_ROUNDS: '1', ARGUS_MODEL_LOCK_DIR: modelLockDir, ARGUS_FOREGROUND_DIR: foregroundDir },
});
// A crash must not leave the engine holding its port for the next run ("Address already in use").
process.on('exit', () => bridge.kill());
const api = (p, init) => fetch(`http://127.0.0.1:7878${p}`, init).then((r) => r.json());
for (let i = 0; ; i++) {
  try {
    await api('/api/list_shoots');
    break;
  } catch (e) {
    if (i > 50) throw e;
    await new Promise((r) => setTimeout(r, 100));
  }
}

const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port: 5198, strictPort: true }, logLevel: 'error' });
await server.listen();
// PW_BROWSER=webkit drives WebKit, the engine behind the Mac app's webview.
const browser = await (process.env.PW_BROWSER === 'webkit' ? webkit.launch() : chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {}));
const failures = [];
const check = (ok, msg) => (ok ? console.log(`ok   ${msg}`) : (failures.push(msg), console.log(`FAIL ${msg}`)));

try {
  const shoots = await api('/api/list_shoots');
  const shoot = shoots[shoots.length - 1];
  const photos = await api(`/api/photos?shoot_id=${shoot.id}`);
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const p = await ctx.newPage();
  // Loading the dev build can take over 30 s when the ui-gate runs every check side by side; that is load, not a stall.
  p.setDefaultNavigationTimeout(90_000);
  // WebKit rejects a fetch that a navigation (p.goto) cancels with "… due to access control checks"
  // or "Load failed": the screen was being left, not broken, so only other page errors fail the run.
  const aborted = /due to access control checks|^TypeError: Load failed$/;
  p.on('pageerror', (e) => aborted.test(e.message) || failures.push(`page error: ${e.message}`));
  const state = async (id) => (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === id);
  // After a key or click: the pause a person would take, then keep asking until `ok` holds or
  // 10 s pass, instead of reading once after a fixed pause (those lost races under ui-gate's
  // load, 2026-11-02). The check that follows reads again and reports what it found.
  const settle = async (ms, ok) => {
    await p.waitForTimeout(ms);
    for (const t0 = Date.now(); !(await ok().catch(() => false)) && Date.now() - t0 < 10000; ) await p.waitForTimeout(150);
  };
  const recipeOf = async (id) => (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: id }) }))?.recipe?.global ?? {};
  const target = photos[photos.length - 1].id;
  const gotoGrid = async () => {
    await p.goto('http://localhost:5198/?library=live#/cull');
    await p.waitForSelector('.tile');
  };
  const gotoEdit = async () => {
    await p.goto(`http://localhost:5198/?library=live#/edit/${target}`);
    await p.waitForSelector('.stage .photo img');
    await p.waitForTimeout(1000);
  };

  if (run('grid')) {
    await p.goto('http://localhost:5198/?library=live#/cull');
    await p.waitForSelector('.tile');
    await p.waitForTimeout(800);
    await p.screenshot({ path: `${out}live-cull.png` });

    const tiles = await p.locator('.tile').count();
    check(tiles === Math.min(photos.length, tiles) && tiles > 0, `grid shows Photos from "${shoot.name}" (${tiles} tiles, ${photos.length} in Library)`);
    const srcs = await p.locator('.tile img').evaluateAll((imgs) => imgs.map((i) => [i.getAttribute('src'), i.naturalWidth]));
    check(srcs.length > 0 && srcs.every(([s, w]) => s.startsWith('/previews/') && w > 0), 'tiles load engine preview JPEGs');
  }

  if (run('flags')) {
    await gotoGrid();
    // Re-read: the Queue worker may have culled since `photos` was read (an app-imported
    // Library has a cull queued), and ⌘Z must put back what was there just before the key.
    const first = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === photos[0].id);
    const want = first.verdict === 'pick' ? 'reject' : 'pick';
    await p.locator(`[data-testid="tile-${first.id}"]`).click();
    await p.keyboard.press(want === 'pick' ? 'p' : 'x');
    await settle(500, async () => { const after = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === first.id); return after.verdict === want && after.verdict_by === 'person'; });
    const after = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === first.id);
    check(after.verdict === want && after.verdict_by === 'person', `pressing ${want === 'pick' ? 'P' : 'X'} wrote ${want} to the Library (got ${after.verdict} by ${after.verdict_by})`);
    await p.screenshot({ path: `${out}live-flagged.png` });

    // The Library is the only source of truth: ⌘Z is the engine's undo_batch, ⇧⌘Z undoes the Undo.
    await p.keyboard.press('ControlOrMeta+z');
    await settle(500, async () => { const unflagged = await state(first.id); return unflagged.verdict === first.verdict && unflagged.verdict_by === first.verdict_by; });
    const unflagged = await state(first.id);
    check(unflagged.verdict === first.verdict && unflagged.verdict_by === first.verdict_by, `⌘Z undid the flag in the Library (got ${unflagged.verdict} by ${unflagged.verdict_by})`);
    await p.keyboard.press('ControlOrMeta+Shift+z');
    await settle(500, async () => { const redone = await state(first.id); return redone.verdict === want && redone.verdict_by === 'person'; });
    const redone = await state(first.id);
    check(redone.verdict === want && redone.verdict_by === 'person', `⇧⌘Z redid it (got ${redone.verdict} by ${redone.verdict_by})`);
    await p.locator(`[data-testid="tile-${first.id}"]`).click(); // P moved on to the next Photo
    await p.keyboard.press('3');
    await settle(500, async () => { const starred = await state(first.id); return starred.stars === 3 && starred.stars_by === 'person'; });
    const starred = await state(first.id);
    check(starred.stars === 3 && starred.stars_by === 'person', `pressing 3 wrote three stars to the Library (got ${starred.stars} by ${starred.stars_by})`);
    // Edit › Undo in the app's menu bar takes the same path as ⌘Z (src/ui/editMenu.ts).
    await p.evaluate(() => window.dispatchEvent(new CustomEvent('argus:edit-menu', { detail: 'undo' })));
    await settle(500, async () => (await state(first.id)).stars === first.stars);
    check((await state(first.id)).stars === first.stars, 'Edit › Undo put the stars back');
    const shown = await p.locator(`[data-testid="tile-${first.id}"]`).getAttribute('data-verdict');
    check(shown === want, `the tile shows what the Library has (${shown})`);
  }

  if (run('zoom')) {
    await gotoGrid();
    // View › Zoom and pinch, which used to need Greg's eyes on the Mac (src/ui/zoom.ts, zoomPan.ts).
    // The menu sends the same event the app's Tauri menu does; pinch is ctrl+wheel in Chromium
    // and gesture events in WebKit, both dispatched straight at the stage.
    const zoomMenu = (step) => p.evaluate((s) => window.dispatchEvent(new CustomEvent('argus:zoom', { detail: s })), step);
    const pageZoom = () => p.evaluate(() => document.documentElement.style.zoom || '1');
    await zoomMenu('in');
    check((await pageZoom()) === '1.1', `View › Zoom In in the grid zooms the app (${await pageZoom()})`);
    await zoomMenu('reset');
    check((await pageZoom()) === '1', `View › Actual Size puts it back (${await pageZoom()})`);
    await p.locator(`[data-testid="tile-${photos[0].id}"]`).click();
    await p.keyboard.press('e');
    await p.waitForSelector('[data-testid="loupe"] img');
    await p.waitForTimeout(400);
    const photoZoom = () => p.evaluate(() => Number([...document.querySelectorAll('[data-testid="loupe"] *')].find((n) => n.style?.getPropertyValue('--zoom'))?.style.getPropertyValue('--zoom') ?? 1));
    await zoomMenu('in');
    await settle(300, async () => (await photoZoom()) > 1 && (await pageZoom()) === '1');
    check((await photoZoom()) > 1 && (await pageZoom()) === '1', `View › Zoom In in Loupe zooms the photo, not the app (photo ${await photoZoom()}, app ${await pageZoom()})`);
    await zoomMenu('reset');
    await settle(300, async () => (await photoZoom()) === 1);
    check((await photoZoom()) === 1, `View › Actual Size fits the photo again (${await photoZoom()})`);
    await p.evaluate(() => {
      const st = document.querySelector('[data-testid="loupe"]');
      const r = st.getBoundingClientRect();
      st.dispatchEvent(new WheelEvent('wheel', { ctrlKey: true, deltaY: -60, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, bubbles: true, cancelable: true }));
    });
    await settle(300, async () => { const pinched = await photoZoom(); return pinched > 1; });
    const pinched = await photoZoom();
    check(pinched > 1, `a trackpad pinch (ctrl+wheel) zooms the photo (${pinched})`);
    await zoomMenu('reset');
    await p.evaluate(() => {
      const st = document.querySelector('[data-testid="loupe"]');
      const r = st.getBoundingClientRect();
      const g = (type, scale) => Object.assign(new Event(type, { bubbles: true, cancelable: true }), { scale, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2 });
      st.dispatchEvent(g('gesturestart', 1));
      st.dispatchEvent(g('gesturechange', 2));
      st.dispatchEvent(g('gestureend', 2));
    });
    await settle(300, async () => (await photoZoom()) >= 1.9);
    check((await photoZoom()) >= 1.9, `a WebKit pinch gesture zooms the photo (${await photoZoom()})`);
    await p.screenshot({ path: `${out}live-loupe-zoomed.png` });
    await zoomMenu('reset');
  }

  if (run('cancel')) {
    await gotoGrid();
    // Cancel in the cull's progress row stops it mid-reply, and the cull changes nothing.
    const before = JSON.stringify((await api(`/api/photos?shoot_id=${shoot.id}`)).map((x) => [x.verdict, x.verdict_by, x.stars, x.stack]));
    await p.getByPlaceholder(/Cull, fix or explain/).fill('cull this shoot');
    await p.getByRole('button', { name: 'Send' }).click();
    await p.getByTestId('cull-progress').getByText(/[1-9]\d* of \d+/).waitFor({ timeout: 10000 });
    await p.locator('.panel.agent').screenshot({ path: `${out}live-cull-cancel.png` }).catch(() => {});
    await p.getByTestId('cull-progress-cancel').click();
    await p.getByText('Cull stopped').waitFor({ timeout: 5000 });
    await p.locator('.panel.agent').screenshot({ path: `${out}live-cull-stopped.png` }).catch(() => {});
    const after = JSON.stringify((await api(`/api/photos?shoot_id=${shoot.id}`)).map((x) => [x.verdict, x.verdict_by, x.stars, x.stack]));
    check(after === before, 'Cancel stopped the AI cull within a moment and changed nothing');
    const tasks = (await api('/api/progress')).tasks ?? [];
    check(!tasks.some((t) => t.kind === 'cull'), 'the cull task is gone once stopped');
  }

  if (run('cull')) {
    await gotoGrid();
    // AI cull from the Agent panel, against the mock endpoint. Clear person flags first,
    // since the cull never overrides them.
    await api('/api/flag', { method: 'POST', body: JSON.stringify({ ids: photos.map((x) => x.id), verdict: 'unflagged', author: 'ai' }) });
    await p.reload();
    await p.waitForSelector('.tile');
    // Count from here: a Library imported in the app has its own cull and Suggestions
    // queued, which the bridge's Queue worker runs at start.
    const callsBefore = mockCalls;
    await p.getByPlaceholder(/Cull, fix or explain/).fill('cull this shoot');
    await p.getByRole('button', { name: 'Send' }).click();
    // The bar moves as the model's answer streams in, Photo by Photo, with an ETA.
    const bar = p.getByTestId('cull-progress');
    await bar.getByText(/[1-9]\d* of \d+/).waitFor({ timeout: 10000 });
    await p.screenshot({ path: `${out}live-cull-progress.png` });
    check(/Culling/.test(await bar.innerText()), 'the cull bar says what the model is doing');
    // A cull started from the chat shows as a foreground task (status-task), not Queue work.
    const work = (await p.locator('[data-testid="status-task"], [data-testid="queue-progress"]').allInnerTexts()).join(' ');
    check(/AI cull/.test(work), 'Background work in the Status bar shows the running cull');
    await p.locator('.status-bar').screenshot({ path: `${out}live-cull-sidebar.png` }).catch(() => {});
    await p.getByText('Mock cull.').waitFor({ timeout: 15000 });
    await p.screenshot({ path: `${out}live-culled.png` });
    const culled = await api(`/api/photos?shoot_id=${shoot.id}`);
    check(mockCalls - callsBefore === 1, 'the Agent sent one Shoot-wide request to the model (Ollama by default)');
    check(culled[0].verdict === 'unflagged' && culled[0].verdict_by === 'ai' && culled[0].reason === 'best of 2', 'AI Verdicts and reasons reached the Library, and the cautious cull left picking to the person');
    check(photos.length < 2 || (culled[0].stack != null && culled[0].stack === culled[1].stack), 'the burst was stacked');
    await p.waitForTimeout(3000); // the panel reloads from the Library
    await p.waitForSelector('.tile');
    await p.screenshot({ path: `${out}live-after-cull.png` });

    // Stacks view (N): Enter keeps the AI's best frame of the burst as your Pick, the rest
    // stay as they were (Unflagged or the AI's Reject), and ⌘Z undoes it in the Library.
    if (photos.length >= 2) {
      await p.keyboard.press('n');
      await p.waitForSelector('.burst-cell');
      await p.waitForTimeout(600);
      await p.screenshot({ path: `${out}live-bursts.png` });
      const stack = culled.filter((x) => x.stack === culled[0].stack).sort((a, b) => a.stack_rank - b.stack_rank);
      await p.keyboard.press('Enter');
      await settle(600, async () => { const kept = (await api(`/api/photos?shoot_id=${shoot.id}`)).filter((x) => stack.some((y) => y.id === x.id)); const best = kept.find((x) => x.id === stack[0].id); return best.verdict === 'pick' && best.verdict_by === 'person' && kept.filter((x) => x.verdict === 'pick').length === 1; });
      const kept = (await api(`/api/photos?shoot_id=${shoot.id}`)).filter((x) => stack.some((y) => y.id === x.id));
      const best = kept.find((x) => x.id === stack[0].id);
      check(best.verdict === 'pick' && best.verdict_by === 'person' && kept.filter((x) => x.verdict === 'pick').length === 1, `Enter in the Stacks view kept the best frame of the burst (got ${kept.map((x) => `${x.verdict}/${x.verdict_by}`)})`);
      await p.keyboard.press('ControlOrMeta+z');
      await settle(500, async () => (await state(stack[0].id)).verdict_by === 'ai');
      check((await state(stack[0].id)).verdict_by === 'ai', '⌘Z undid keeping it');
      await p.keyboard.press('Escape');
      await p.waitForSelector('.tile');
    }
  }

  if (run('auto')) {
    await gotoGrid();
    // Auto all: a progress row in the chat while the engine measures each Photo, the same
    // in Background work, then the result with Undo (Greg, 2026-10-21).
    const before = await Promise.all(photos.map((x) => recipeOf(x.id)));
    await p.getByTestId('auto-all').click();
    // Auto all opens the Apply popover with each Photo measured; Apply runs it.
    await p.getByTestId('apply-edit-go').waitFor({ timeout: 10000 });
    await p.getByTestId('apply-edit-reading').waitFor({ state: 'detached', timeout: 120000 }).catch(() => {});
    // Auto all skips Rejects, and Photos edited by hand start left out (36f08fb): only the ticked tiles change.
    const ticked = (await p.getByTestId('apply-edit-tile').evaluateAll((els) => els.filter((e) => e.getAttribute('aria-pressed') !== 'false').map((e) => e.getAttribute('aria-label') ?? ''))).map((l) => l.split(',')[0]);
    await p.getByTestId('apply-edit-go').click();
    const row = p.getByTestId('auto-progress');
    // The row can come and go between the wait and the read on a slow box (Mac thread,
    // 2026-11-02: innerText timed out): read it once, short, and accept a run that already ended.
    await row.waitFor({ timeout: 10000 }).catch(() => {});
    const rowText = await row.innerText({ timeout: 2000 }).catch(() => '');
    const endedFirst = !rowText && (await p.getByText(/Auto applied to \d+ Photos?/).first().isVisible());
    check(/Auto on \d+ Photos?/.test(rowText) || endedFirst, `Auto all shows a progress row in the chat (${rowText ? 'row read' : endedFirst ? 'finished before it could be read' : 'none'})`);
    await row.getByText(/\d+ of \d+/).waitFor({ timeout: 10000 }).catch(() => {});
    await p.locator('.panel.agent').screenshot({ path: `${out}live-auto-progress.png` }).catch(() => {});
    // A fast machine can finish 3 Photos before the sidebar is read: poll, and only let
    // that pass when Auto was already done.
    let side = '';
    let finished = false;
    for (let i = 0; i < 50; i++) {
      side = (await p.locator('[data-testid="status-task"], [data-testid="queue-progress"]').allInnerTexts()).join(' ');
      if (/Auto/.test(side)) break;
      finished = await p.getByText(/Auto applied to \d+ Photos?/).first().isVisible();
      if (finished) break;
      await p.waitForTimeout(100);
    }
    await p.locator('.status-bar').screenshot({ path: `${out}live-auto-sidebar.png` }).catch(() => {});
    check(/Auto/.test(side) || finished, `Background work shows Auto while it runs (got "${side.replace(/\n/g, ' | ')}"${finished ? ', Auto already done' : ''})`);
    await p.getByText(/Auto applied to \d+ Photos?/).first().waitFor({ timeout: 120000 });
    await p.locator('.panel.agent').screenshot({ path: `${out}live-auto-done.png` }).catch(() => {});
    const after = await Promise.all(photos.map((x) => recipeOf(x.id)));
    const isIn = (i) => ticked.includes(photos[i].file_name);
    const wrong = photos.filter((x, i) => (JSON.stringify(after[i]) !== JSON.stringify(before[i])) !== isIn(i)).map((x) => x.file_name);
    check(ticked.length > 0 && !wrong.length, `Auto all changed every ticked Photo and nothing else (ticked: ${ticked.join(', ')}; wrong: ${wrong.join(', ') || 'none'})`);
    await p.locator('.panel.agent').getByRole('button', { name: 'Undo' }).last().click();
    await settle(1500, async () => { const undone = await Promise.all(photos.map((x) => recipeOf(x.id))); return undone.every((g, i) => JSON.stringify(g) === JSON.stringify(before[i])); });
    const undone = await Promise.all(photos.map((x) => recipeOf(x.id)));
    check(undone.every((g, i) => JSON.stringify(g) === JSON.stringify(before[i])), 'Undo in the chat put every Recipe back');
  }

  if (run('export')) {
    await gotoGrid();
    // Export the Picks to a folder chosen in the dialog (a prompt in a browser), with an
    // XMP-free JPEG preset, and check the files land named by Shoot.
    const exportDir = mkdtempSync(join(tmpdir(), 'argus-export-'));
    // The AI never picks (cautious cull), so the person picks the best frame for the rest.
    await api('/api/flag', { method: 'POST', body: JSON.stringify({ ids: [photos[0].id], verdict: 'pick', author: 'person' }) });
    await p.reload();
    await p.waitForSelector('.tile');
    const picks = (await api(`/api/photos?shoot_id=${shoot.id}`)).filter((x) => x.verdict === 'pick').length;
    await p.keyboard.press('ControlOrMeta+e');
    p.once('dialog', (d) => d.accept(exportDir));
    await p.getByRole('button', { name: 'Choose…' }).click();
    await p.getByRole('button', { name: /^Web/ }).click();
    await p.getByTestId('export-start').click();
    await p.getByText(/^Done\. /).waitFor({ timeout: 60000 });
    await p.waitForTimeout(300); // let the bar finish its transition
    await p.screenshot({ path: `${out}live-exported.png` });
    const shootDir = readdirSync(exportDir);
    const files = shootDir.length === 1 ? readdirSync(join(exportDir, shootDir[0])) : [];
    check(files.length === picks && files.every((f) => f.endsWith('.jpg') && !f.includes(' ') && f.startsWith(shootDir[0] + '_')), `Export wrote ${picks} Picks into a Shoot folder (${shootDir.join(', ')}: ${files.join(', ')})`);
    await p.keyboard.press('Escape');
  }

  if (run('agent')) {
    // The Agent edits the Photo open in Edit, as one undoable Version.
    await p.goto(`http://localhost:5198/?library=live#/edit/${target}`);
    await p.getByPlaceholder(/A bit more on the face/).fill('warmer, less contrast');
    await p.getByRole('button', { name: 'Send' }).click();
    await p.getByText('Mock edit: Temperature +12, Contrast -15.').waitFor({ timeout: 15000 });
    const edited = await recipeOf(target);
    check(agentCalls === 2 && edited.temp === 12 && edited.contrast === -15, `the Agent's edit reached the Library (${JSON.stringify(edited)}, ${agentCalls} model turns)`);
    await p.waitForTimeout(1500); // the render lands; nothing is saved back as a person Version
    await p.getByRole('tab', { name: /History/ }).click();
    await p.getByText('Agent: warmer, less contrast').waitFor({ timeout: 5000 });
    const versions = await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    // The auto step above leaves its own batch and its Undo in History; leave those out.
    const own = versions.filter((v) => !/Auto on \d+ Photos?/.test(v.label));
    check(own.length === 1 && own[0].author === 'ai', `History shows the Agent's Version and nothing else (${versions.map((v) => v.label).join(', ')})`);
    await p.screenshot({ path: `${out}live-agent-edit.png` });
    await p.locator('.agent').getByRole('button', { name: 'Undo', exact: true }).click();
    await settle(1000, async () => { const undone = await recipeOf(target); return !undone.temp && !undone.contrast; });
    const undone = await recipeOf(target);
    check(!undone.temp && !undone.contrast, `Undo in the chat put the Recipe back (${JSON.stringify(undone)})`);
    await p.screenshot({ path: `${out}live-agent-undo.png` });
  }

  if (run('agent-batch')) {
    // An Agent edit on several Photos opens the Apply popover on them (Greg, 2026-11-02):
    // untick one and Return puts it back; Esc on a second turn puts them all back.
    batchIds = (await api('/api/list_photos', { method: 'POST', body: '{}' })).slice(0, 3).map((x) => x.id);
    await p.goto('http://localhost:5198/?library=live#/cull');
    await p.waitForSelector('.tile');
    const ask = async () => {
      const box = p.locator('textarea').last();
      await box.fill('warm every photo');
      await box.press('Enter');
      await p.getByTestId('apply-edit').waitFor({ timeout: 20000 });
      await p.getByTestId('apply-edit-reading').waitFor({ state: 'detached', timeout: 120000 }).catch(() => {});
    };
    const temps = async () => Promise.all(batchIds.map(async (id) => (await recipeOf(id)).temp ?? 0));
    await ask();
    const n = await p.getByTestId('apply-edit-tile').count();
    check(n === batchIds.length, `the Agent's batch edit opens the Apply popover on its ${batchIds.length} Photos (${n})`);
    const drawnN = await p.locator('[data-testid="apply-edit-tile"][data-drawn="yes"]').count();
    check(drawnN === batchIds.length, `the engine draws each as the Agent left it (${drawnN})`);
    await p.screenshot({ path: `${out}live-agent-batch.png` });
    // Untick the last tile, and find its Photo by name: the popover's order needn't match list_photos.
    const offName = ((await p.getByTestId('apply-edit-tile').last().getAttribute('aria-label')) ?? '').split(',')[0];
    const files = await Promise.all(batchIds.map(async (id) => (await api('/api/list_photos', { method: 'POST', body: '{}' })).find((x) => x.id === id)?.file_name));
    const off = files.indexOf(offName);
    await p.getByTestId('apply-edit-tile').last().click();
    await p.keyboard.press('Enter');
    await p.getByTestId('apply-edit').waitFor({ state: 'detached', timeout: 5000 });
    let t = [];
    for (let i = 0; i < 50; i++) {
      t = await temps();
      if (t[off] === 0) break;
      await p.waitForTimeout(200);
    }
    check(off >= 0 && t.every((x, i) => (i === off ? x === 0 : x > 0)), `Return keeps the ticked edits and puts the unticked Photo back (unticked ${offName}; temps ${t.join(', ')})`);
    const before = await temps();
    await ask();
    await p.keyboard.press('Escape');
    await p.getByTestId('apply-edit').waitFor({ state: 'detached', timeout: 5000 });
    let back = [];
    for (let i = 0; i < 50; i++) {
      back = await temps();
      if (back.every((x, j) => x === before[j])) break;
      await p.waitForTimeout(200);
    }
    check(back.every((x, j) => x === before[j]), `Esc puts every Photo back as it was before the request (${back.join(', ')})`);
  }

  if (run('sliders')) {
    await gotoEdit();
    // A slider drag is one engine batch: ⌘Z is undo_batch on it, ⇧⌘Z undoes the Undo.
    const exposure = async () => (await recipeOf(target)).exposure ?? 0;
    await p.getByRole('tab', { name: 'Edit', exact: true }).click(); // History is open
    const thumb = p.getByRole('slider', { name: 'Exposure', exact: true });
    const versionCount = async () => (await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) })).length;
    // Wait for what the check is about, not a fixed time: a busy box (ui-gate runs five checks
    // side by side) took longer than the old fixed sleeps and failed these checks at random.
    const until = async (ok, ms = 10000) => {
      for (let t = 0; t < ms; t += 200) {
        if (await ok()) return true;
        await p.waitForTimeout(200);
      }
      return false;
    };
    const nudge = async (n) => {
      const had = await versionCount();
      await thumb.focus();
      for (let i = 0; i < n; i++) await p.keyboard.press('ArrowRight');
      await p.waitForTimeout(1700); // the drag rests, then saves
      await until(async () => (await versionCount()) > had);
    };
    const versionsBefore = await versionCount();
    await nudge(4);
    const dragged = await exposure();
    await nudge(2);
    const second = await exposure();
    const drags = (await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) })).slice(versionsBefore);
    check(dragged > 0 && second > dragged && drags.length === 2 && drags.every((v) => v.author === 'person' && /^Exposure /.test(v.label ?? '')), `two Exposure drags saved as two Versions (${drags.map((v) => v.label).join(', ')})`);
    await p.locator('.stage').click();
    await p.keyboard.press('ControlOrMeta+z');
    await until(async () => (await exposure()) === dragged);
    check((await exposure()) === dragged, `⌘Z undid the last drag in the Library (exposure ${await exposure()}, want ${dragged})`);
    await p.keyboard.press('ControlOrMeta+z');
    await until(async () => (await exposure()) === 0);
    check((await exposure()) === 0, `⌘Z again undid the first drag (exposure ${await exposure()})`);
    await p.keyboard.press('ControlOrMeta+Shift+z');
    await until(async () => (await exposure()) === dragged);
    check((await exposure()) === dragged, `⇧⌘Z redid it (exposure ${await exposure()})`);
    const exposureSlider = p.locator('.slider').filter({ has: p.locator('.slider-label', { hasText: /^Exposure$/ }) });
    await until(async () => !!(await exposureSlider.textContent())?.includes(dragged.toFixed(2)));
    const label = await exposureSlider.textContent();
    check(label?.includes(dragged.toFixed(2)), `the slider shows what the Library has (${label})`);
  }

  if (run('suggest')) {
    await gotoEdit();
    // Suggestions: R queues new takes, the bridge's Queue worker asks the model, and the
    // takes show up first in the Edit panel; accepting one saves it as a Version.
    await p.locator('.stage').click();
    const suggestBefore = suggestCalls;
    await p.keyboard.press('r');
    await p.locator('.take-name', { hasText: 'Punchy' }).waitFor({ timeout: 90000 }); // the Queue runs the takes behind other work; 30 s ran out under heavy load (2026-11-02)
    // The takes may already show from the start-up Suggestions; wait for R's own requests.
    for (let i = 0; i < 60 && suggestCalls - suggestBefore < 2; i++) await p.waitForTimeout(500);
    await p.waitForTimeout(1000);
    // One proposal plus one Review round (pinned above, the Full default).
    check(suggestCalls - suggestBefore === 2, `the Queue asked the model for takes and one Review round (${suggestCalls - suggestBefore})`);
    // Hovering a take previews it on the photo; leaving puts your edit back (Greg, 2026-11-01).
    await p.getByTestId('take').filter({ hasText: 'Punchy' }).hover();
    await p.getByTestId('previewing').getByText(/Punchy/).waitFor({ timeout: 5000 });
    await p.locator('.stage').hover();
    await p.getByTestId('previewing').waitFor({ state: 'detached', timeout: 5000 });
    check(true, 'hovering a take previews it, leaving puts your edit back');
    await p.keyboard.press('Tab');
    await p.keyboard.press('Tab');
    await p.getByTestId('previewing').getByText(/Punchy/).waitFor({ timeout: 5000 });
    await p.waitForTimeout(800); // the take's render
    await p.screenshot({ path: `${out}live-suggestions.png` });
    await p.keyboard.press('Enter');
    await settle(1200, async () => { const accepted = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }); return accepted?.recipe?.global?.exposure === 0.7 && accepted?.recipe?.crop?.w === 0.8; });
    const accepted = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    check(accepted?.recipe?.global?.exposure === 0.7 && accepted?.recipe?.crop?.w === 0.8, `accepting a take saved it with its crop (${JSON.stringify(accepted?.recipe)})`);
    const history = await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    check(history.at(-1)?.label === 'Suggestion: Punchy', `the take is a Version of its own (${history.at(-1)?.label})`);
  }

  if (run('masks')) {
    // Edit: a Sky Mask is rendered by the engine and its real Mask is tinted on top.
    await p.goto(`http://localhost:5198/?library=live#/edit/${target}`);
    await p.waitForSelector('.stage .photo img');
    await p.waitForTimeout(1500);
    const before = await p.locator('.stage .photo img').first().getAttribute('src');
    const cropW = (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }))?.recipe?.crop?.w;
    // Auto: the engine measures the Photo and saves Lightroom's Auto sliders as one batch.
    await p.getByRole('tab', { name: 'Edit', exact: true }).click(); // History may still be open
    await p.getByTestId('auto').click();
    await p.waitForFunction((b) => document.querySelector('.stage .photo img')?.getAttribute('src') !== b, before, { timeout: 20000 });
    const autoed = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    const autoKeys = ['exposure', 'contrast', 'highlights', 'shadows', 'whites', 'blacks', 'temp', 'tint', 'vibrance'].filter((k) => autoed?.recipe?.global?.[k]);
    check(autoKeys.length >= 3 && autoed?.recipe?.crop?.w === cropW, `Auto set ${autoKeys.join(', ')} and kept the crop`);
    const afterAuto = await p.locator('.stage .photo img').first().getAttribute('src');
    // A preset is the person's own edit: its sliders reach the Library, the rest is kept.
    await p.getByRole('tab', { name: 'Presets' }).click();
    // Film is closed by default and remembers being opened; Suggested may list Warm film too.
    const film = p.getByTestId('section-presets-Film');
    if ((await film.locator('.dev-section-toggle').getAttribute('aria-expanded')) !== 'true') await film.locator('.dev-section-toggle').click();
    await film.getByTestId('preset-warm-film').click();
    await settle(1500, async () => { const preset = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }); const warmed = (preset?.recipe?.global?.temp ?? 0) - (autoed?.recipe?.global?.temp ?? 0); return warmed === 12 && preset?.recipe?.presets?.[0]?.id === 'warm-film' && preset?.recipe?.crop?.w === cropW; });
    const preset = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    const warmed = (preset?.recipe?.global?.temp ?? 0) - (autoed?.recipe?.global?.temp ?? 0);
    check(warmed === 12 && preset?.recipe?.presets?.[0]?.id === 'warm-film' && preset?.recipe?.crop?.w === cropW, `the Warm film preset was saved on top of Auto (temp +${warmed}, remembered as ${preset?.recipe?.presets?.[0]?.id})`);
    // The first look at the Presets asks the engine to rank them for the Shoot in the background.
    const ranking = await api('/api/list_jobs', { method: 'POST', body: JSON.stringify({ kind: 'rank_presets', limit: 5 }) });
    check(ranking?.some((j) => j.args?.shoot_id === shoot.id), 'opening Presets queued rank_presets for the Shoot');
    const afterPreset = await p.locator('.stage .photo img').first().getAttribute('src');
    await p.getByRole('tab', { name: 'Masking' }).click();
    await p.getByRole('button', { name: 'Sky', exact: true }).click();
    await p.waitForFunction((b) => document.querySelector('.stage .photo img')?.getAttribute('src') !== b, afterPreset, { timeout: 20000 });
    check(true, 'adding a Sky Mask re-rendered the Photo through the engine');
    // The Masking tool shows the overlay by itself.
    const tint = await p.waitForSelector('[data-testid="mask-tint"]', { timeout: 20000 }).then(() => true, () => false);
    const style = tint ? await p.locator('[data-testid="mask-tint"]').first().getAttribute('style') : '';
    check(tint && /data:image\/png/.test(style ?? ''), "Masking tints the engine's own Mask (view_mask)");
    await p.waitForTimeout(800);
    await p.screenshot({ path: `${out}live-edit-masks.png` });
    const saved = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    check(saved?.recipe?.local?.some((l) => l.mask.kind === 'sky'), 'the Sky Mask was saved in the Recipe');
    // Select People: the engine finds who is there (analyze_photo) and saves a person Mask.
    await p.getByTestId('mask-create').getByRole('button', { name: 'People' }).click();
    const found = await p.getByTestId('people-picker').getByText(/found\./).waitFor({ timeout: 30000 }).then(() => true, () => false);
    await p.waitForTimeout(1500);
    await p.screenshot({ path: `${out}live-people.png` });
    const person = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    check(found && person?.recipe?.local?.some((l) => l.mask.kind === 'person' && l.mask.parts.includes('face_skin')), `the people picker asked the engine and a person Mask was saved (${await p.getByTestId('people-picker').locator('.people-note').textContent().catch(() => '?')})`);
    // A suggested Look from Presets and a one-click fix from the Edit tool reach the Library.
    await p.getByRole('tab', { name: 'Presets' }).click();
    const recId = (await p.getByTestId('presets-suggested').locator('[data-testid^="preset-"]').first().getAttribute('data-testid')).slice(7);
    await p.getByTestId('presets-suggested').getByTestId(`preset-${recId}`).click();
    await p.getByRole('tab', { name: 'Edit', exact: true }).click();
    await p.getByTestId('fix-shadows').click();
    await settle(1500, async () => { const rec = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }); const on = (rec?.recipe?.presets ?? []).map((x) => x.id); return on.includes(recId) && on.includes('fix-shadows') && !on.includes('warm-film'); });
    const rec = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
    const on = (rec?.recipe?.presets ?? []).map((x) => x.id);
    check(on.includes(recId) && on.includes('fix-shadows') && !on.includes('warm-film'), `a suggested Look replaced Warm film and Lift shadows stacked on it (${on.join(', ')})`);
  }

  if (run('look')) {
    // The Look screen runs on the Library's Looks: a slider drag and Remove Look are batches
    // of the person's own, and ⌘Z / ⇧⌘Z are undo_batch.
    if (!(await api(`/api/photos?shoot_id=${shoot.id}`)).some((x) => x.verdict === 'pick')) {
      await api('/api/flag', { method: 'POST', body: JSON.stringify({ ids: [photos[0].id], verdict: 'pick', author: 'person' }) });
    }
    const pick = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.verdict === 'pick');
    const made = await api('/api/set_look', { method: 'POST', body: JSON.stringify({ shoot_id: shoot.id, name: 'Test look', photo_ids: [pick.id] }) });
    const lookContrast = async () => (await api('/api/list_looks', { method: 'POST', body: JSON.stringify({ shoot_id: shoot.id }) }))[0]?.adjust?.contrast ?? 0;
    const lookOf = async () => (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: pick.id }) }))?.recipe?.look ?? null;
    await p.goto('http://localhost:5198/?library=live#/look');
    await p.getByRole('heading', { name: 'Look: Test look' }).waitFor({ timeout: 10000 });
    check(true, 'the Look screen shows the Library\'s Look');
    await p.getByRole('button', { name: 'Adjust' }).click();
    await p.getByRole('slider', { name: 'Contrast', exact: true }).focus();
    for (let i = 0; i < 5; i++) await p.keyboard.press('ArrowRight');
    await settle(1700, async () => (await lookContrast()) === 5);
    check((await lookContrast()) === 5, `a Look slider drag reached the Library (contrast ${await lookContrast()})`);
    await p.locator(`[data-testid="batch-${pick.id}"]`).click();
    await p.keyboard.press('ControlOrMeta+z');
    await settle(800, async () => (await lookContrast()) === 0);
    check((await lookContrast()) === 0, `⌘Z undid the Look drag in the Library (contrast ${await lookContrast()})`);
    await p.keyboard.press('ControlOrMeta+Shift+z');
    await settle(800, async () => (await lookContrast()) === 5);
    check((await lookContrast()) === 5, `⇧⌘Z redid it (contrast ${await lookContrast()})`);
    await p.getByRole('button', { name: 'Remove Look' }).click();
    await settle(800, async () => (await lookOf()) === null);
    check((await lookOf()) === null, `Remove Look took the Pick out from under it (${await lookOf()})`);
    await p.locator(`[data-testid="batch-${pick.id}"]`).click();
    await p.keyboard.press('ControlOrMeta+z');
    await p.getByRole('heading', { name: 'Look: Test look' }).waitFor({ timeout: 5000 }).catch(() => {});
    check((await lookOf()) === made.look.id, `⌘Z put the Look back (${await lookOf()})`);
    check(await p.getByRole('heading', { name: 'Look: Test look' }).isVisible(), 'the Look screen shows it again');
    await p.screenshot({ path: `${out}live-look.png` });
  }

  if (run('rename')) {
    // Renaming the Shoot from the sidebar goes through rename_shoot: the stored name follows
    // the naming rules and every Photo still finds its original.
    await p.goto('http://localhost:5198/?library=live#/cull');
    await p.waitForSelector('.tile');
    const row = p.getByTestId(`shoot-${shoot.id}`);
    await row.dblclick();
    await p.keyboard.press('ControlOrMeta+a');
    await p.keyboard.type('2026-10-10 Boat tour');
    await p.keyboard.press('Enter');
    await settle(500, async () => { const renamed = (await api('/api/list_shoots')).find((s) => s.id === shoot.id); return renamed.name === '2026-10-10_Boat-tour'; });
    const renamed = (await api('/api/list_shoots')).find((s) => s.id === shoot.id);
    check(renamed.name === '2026-10-10_Boat-tour', `renaming in the sidebar stored the Shoot as ${renamed.name}`);
    check((await row.textContent()).includes('2026-10-10_Boat-tour'), 'the sidebar shows the new name');
    const missing = await api('/api/find_missing_originals', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' });
    check(Array.isArray(missing.missing ?? missing) && (missing.missing ?? missing).length === 0, 'no original went missing after the rename');
  }

  if (run('remove')) {
    await gotoGrid();
    // Remove from Library: ⌫ asks, Remove takes the Photo out of the Library (originals stay),
    // ⌘Z right after puts it back with its flag.
    const inShoot = await api(`/api/photos?shoot_id=${shoot.id}`);
    const victim = inShoot[0];
    await p.locator(`[data-testid="tile-${victim.id}"]`).click();
    await p.keyboard.press('Backspace');
    await p.getByTestId('remove-dialog').waitFor();
    await p.screenshot({ path: `${out}live-remove-ask.png` });
    await p.getByTestId('remove-confirm').click();
    await settle(600, async () => { const without = await api(`/api/photos?shoot_id=${shoot.id}`); return without.length === inShoot.length - 1 && !without.some((x) => x.id === victim.id); });
    const without = await api(`/api/photos?shoot_id=${shoot.id}`);
    check(without.length === inShoot.length - 1 && !without.some((x) => x.id === victim.id), `Remove took ${victim.file_name} out of the Library (${without.length} left)`);
    check(existsSync(victim.path), 'its original is still on disk');
    check((await p.locator(`[data-testid="tile-${victim.id}"]`).count()) === 0, 'the grid dropped it');
    await p.screenshot({ path: `${out}live-removed.png` });
    await p.keyboard.press('ControlOrMeta+z');
    await settle(800, async () => { const back = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === victim.id); return back && back.verdict === victim.verdict; });
    const back = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === victim.id);
    check(back && back.verdict === victim.verdict, `⌘Z put it back with its Verdict (${back?.verdict})`);
    check((await p.locator(`[data-testid="tile-${victim.id}"]`).count()) === 1, 'the grid shows it again');
  }

  if (run('keywords')) {
    // Keywords and caption in the loupe, saved by the engine (read_tags), as the person's own.
    await gotoGrid();
    const id = photos[0].id;
    await p.locator(`[data-testid="tile-${id}"]`).click();
    await p.keyboard.press('e');
    await p.waitForSelector('[data-testid="keywords"]');
    await p.keyboard.press('k');
    await p.keyboard.type('Places > Portugal > Lisbon');
    await p.waitForTimeout(300);
    await p.keyboard.press('Enter');
    await p.keyboard.press('Escape');
    await p.getByLabel('Caption').fill('Lunch in Alfama.');
    await p.getByLabel('Caption').blur();
    await settle(500, async () => { const tags = (await api('/api/read_tags', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ photo_ids: [Number(id)] }) }))[0]; return tags?.keywords?.some((k) => k.keyword === 'Places|Portugal|Lisbon' && !k.suggested); });
    const tags = (await api('/api/read_tags', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ photo_ids: [Number(id)] }) }))[0];
    check(tags?.keywords?.some((k) => k.keyword === 'Places|Portugal|Lisbon' && !k.suggested), `K adds a kept Keyword in the engine (${JSON.stringify(tags?.keywords)})`);
    check(tags?.caption?.text === 'Lunch in Alfama.' && !tags.caption.suggested, `the caption is saved as the person's (${JSON.stringify(tags?.caption)})`);
    check((await p.getByText('Portugal › Lisbon').count()) > 0, 'the new Keyword shows as a chip');
    await p.screenshot({ path: `${out}live-keywords.png` });
  }
  if (run('lensblur')) {
    // Lens Blur: the Amount slider saves to the Recipe, the engine finds depth (Depth Anything
    // V2) and the blurred render replaces the photo.
    await gotoEdit();
    await p.getByRole('tab', { name: 'Edit', exact: true }).click();
    const sec = p.getByTestId('section-lensblur');
    if ((await sec.locator('.dev-section-toggle').getAttribute('aria-expanded')) !== 'true') await sec.locator('.dev-section-toggle').click();
    const before = await p.locator('.stage .photo img').first().getAttribute('src');
    await sec.getByRole('slider', { name: 'Lens Blur amount' }).focus();
    for (let i = 0; i < 10; i++) await p.keyboard.press('ArrowRight');
    await settle(1700, async () => { const lb = (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }))?.recipe?.lensBlur; return lb?.amount > 0; });
    const lb = (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) }))?.recipe?.lensBlur;
    check(lb?.amount > 0, `the Lens Blur Amount saved to the Recipe (${JSON.stringify(lb)})`);
    const depth = await p.getByTestId('depth-bar').waitFor({ timeout: 60000 }).then(() => true, () => false);
    check(depth, 'the engine found depth and the focal range shows it');
    const changed = await p.waitForFunction((b) => document.querySelector('.stage .photo img')?.getAttribute('src') !== b, before, { timeout: 60000 }).then(() => true, () => false);
    check(changed, 'the photo re-rendered with Lens Blur');
    await p.screenshot({ path: `${out}live-lens-blur.png` });
  }
  if (run('people')) {
    // People: the face models read the Shoot in the Queue, and list_people groups the faces.
    // This throwaway Library's Queue must run on battery too: on an unplugged Mac it otherwise
    // waits for the charger and the face read never starts (2026-11-02, release-candidate).
    await api('/api/set_queue', { method: 'POST', body: JSON.stringify({ run_on_battery: true }) });
    const queued = await api('/api/read_faces', { method: 'POST', body: '{}' });
    let people = await api('/api/list_people', { method: 'POST', body: '{}' });
    // The Queue reaches the face read after the Shoot's earlier jobs (pre-edit, Shoot match), which
    // can take a few minutes on a busy Mac, so allow up to 4 minutes.
    for (let i = 0; i < 240 && people?.unread_photos > 0; i++) {
      await p.waitForTimeout(1000);
      people = await api('/api/list_people', { method: 'POST', body: '{}' });
    }
    check(queued != null && people != null && people.unread_photos === 0, `the face models read every Photo for People (${JSON.stringify(people)?.slice(0, 160)})`);
  }
  if (run('search')) {
    // ⌘K finds Photos by their Keywords across the Library, ⌘↵ opens them all in the grid,
    // and the search saves as an Album in the sidebar.
    await gotoGrid();
    // Two Photos from different Stacks: the grid shows a Stack as one tile, and the AI cull
    // above stacks the burst.
    const now = await api(`/api/photos?shoot_id=${shoot.id}`);
    const ids = now.filter((x, i) => x.stack == null || now.findIndex((y) => y.stack === x.stack) === i).slice(0, 2).map((x) => Number(x.id));
    await api('/api/set_keywords', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ photo_ids: ids, add: ['Activities|Sports|pickleball'] }) });
    await p.keyboard.press('ControlOrMeta+k');
    await p.keyboard.type('pickle');
    await p.waitForSelector('[data-testid="palette-show-all"]', { timeout: 5000 }).catch(() => {});
    check((await p.locator('.found-thumb').count()) >= 2, `⌘K shows the Photos it found (${await p.locator('.found-thumb').count()})`);
    await p.screenshot({ path: `${out}live-search-palette.png` });
    await p.keyboard.press('ControlOrMeta+Enter');
    await p.waitForURL(/#\/cull/).catch(() => {});
    // The search grid asks the engine for the matches; under the release gate's load (five
    // checks side by side on four cores) that took over 15 s once, so wait for its header.
    await p.waitForSelector('.found-line', { timeout: 30000 }).catch(() => {});
    await p.waitForSelector('[data-testid^="tile-"]', { timeout: 30000 }).catch(() => {});
    await settle(500, async () => { const tiles = await p.locator('[data-testid^="tile-"]').count(); return tiles >= 2 && tiles < photos.length + 1; });
    const tiles = await p.locator('[data-testid^="tile-"]').count();
    check(tiles >= 2 && tiles < photos.length + 1, `⌘↵ opens the matches in the grid (${tiles} tiles)`);
    check(/in 1 Shoot/.test(await p.locator('.found-line').innerText().catch(() => '')), 'the header says how many it found');
    await p.getByTestId('save-album').click();
    await p.waitForSelector('[data-testid="nav-album"]', { timeout: 15000 }).catch(() => {});
    check((await p.getByTestId('nav-album').innerText().catch(() => '')).includes('Pickle'), 'Save as Album adds it to the sidebar');
    await p.screenshot({ path: `${out}live-search-album.png` });
  }
  if (run('views')) {
    await gotoGrid();
    // Library-wide views from the sidebar: every Photo, then every Pick.
    const everything = await api('/api/photos');
    await p.getByTestId('nav-all').click();
    await p.waitForSelector('.tile');
    await settle(600, async () => (await p.locator('h1').first().textContent()) === 'All photos');
    check((await p.locator('h1').first().textContent()) === 'All photos', 'All photos opens a Library-wide grid');
    check((await p.locator('[data-testid="ai-cull"]').count()) === 0, 'the Library-wide grid hides the Shoot-only AI cull');
    await p.getByTestId('nav-picks').click();
    await p.waitForSelector('[data-testid="nav-picks"].active');
    await settle(600, async () => { const picksShown = Number(await p.getByTestId('nav-picks').locator('.count').textContent()); return picksShown === everything.filter((x) => x.verdict === 'pick').length; });
    const picksShown = Number(await p.getByTestId('nav-picks').locator('.count').textContent());
    check(picksShown === everything.filter((x) => x.verdict === 'pick').length, `Picks shows every Pick in the Library (${picksShown})`);
    await p.screenshot({ path: `${out}live-picks.png` });
  }

  if (run('backup')) {
    // Settings › Storage and backup on the real Library: the parts table, then a Backup
    // place added through "Another folder…", backed up on the Queue, and its copy on disk.
    await gotoGrid();
    const disk = join(mkdtempSync(join(tmpdir(), 'argus-backup-')), 'disk');
    mkdirSync(disk);
    p.once('dialog', (d) => void d.accept(disk));
    await p.keyboard.press('Control+Comma');
    await p.getByTestId('settings-storage').click();
    await p.getByTestId('storage-settings').waitFor();
    check((await p.getByTestId('part-db').innerText()).includes('library.db'), 'Storage lists the Library database');
    await p.getByTestId('add-backup').click();
    await p.getByTestId('add-backup-place').waitFor();
    await p.getByText('Another folder…').click();
    await p.getByTestId('backup-estimate').waitFor({ timeout: 10000 });
    await p.getByTestId('add-backup-go').click();
    await p.getByTestId('backup-place').first().waitFor();
    let st;
    for (let i = 0; i < 60; i++) {
      st = await api('/api/backup_places', { method: 'POST', body: '{}' });
      if (st.places?.[0]?.last_ok) break;
      await p.waitForTimeout(1000);
    }
    const place = st.places?.[0];
    check(place?.last_ok && place.waiting === 0, `the first backup ran on the Queue (${place?.status}, ${place?.photos_in} Photos)`);
    check(existsSync(join(place?.copy_dir ?? disk, 'library.db')), 'the copy holds library.db');
    await p.getByTestId('backup-place').first().click();
    await p.getByTestId('backup-detail').waitFor();
    // The page asks again every 3 s.
    await p.getByTestId('backup-detail').getByText('First backup').waitFor({ timeout: 8000 }).catch(() => {});
    check((await p.getByTestId('backup-detail').innerText()).includes('First backup'), "the place's history shows the first backup");
    await p.screenshot({ path: `${out}live-backup.png` });
  }

  await ctx.close();
} finally {
  await browser.close();
  await server.close();
  bridge.kill();
  mock.close();
}
if (failures.length) {
  console.error(`\n${failures.length} live check(s) failed:\n${failures.join('\n')}`);
  process.exit(1);
}
console.log('\nlive: all checks passed');
