// Live smoke test: the prototype against a real Library through `argus serve`.
// Works on a temporary copy of the Library, so the original is never changed.
// Starts the bridge and Vite, opens ?library=live, checks every Photo shows its
// engine preview, flags one with X or P and confirms the Verdict reached the Library
// (and that ⌘Z, ⇧⌘Z and star keys are engine writes too),
// then asks the Agent to cull against a mock Claude endpoint and checks the AI
// Verdicts and Stacks land. Then it opens a Photo in Edit, asks the Agent for "warmer,
// less contrast" (the mock answers with an edit_recipe call), checks the new Recipe
// Version and its History row, and undoes it from the chat. Between the two it exports
// the Picks through the Export dialog and checks the files. Last it adds a Sky Mask and checks the
// engine renders it and the overlay shows the engine's own Mask. Screenshots land in
// out/shots/live-*.png.
//
//   node scripts/live.mjs <library-dir>     (build one with `argus import <folder> --library <dir>`)
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

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7878'], {
  stdio: ['ignore', 'inherit', 'inherit'],
  // Full: never throttle (pause) a real Ollama on this machine during a smoke test.
  env: { ...process.env, ARGUS_PROVIDER: 'ollama', OLLAMA_HOST: '127.0.0.1:7879', ARGUS_COMPUTE: 'full', ARGUS_REFINE_ROUNDS: '1' },
});
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
  p.on('pageerror', (e) => failures.push(`page error: ${e.message}`));
  await p.goto('http://localhost:5198/?library=live#/cull');
  await p.waitForSelector('.tile');
  await p.waitForTimeout(800);
  await p.screenshot({ path: `${out}live-cull.png` });

  const tiles = await p.locator('.tile').count();
  check(tiles === Math.min(photos.length, tiles) && tiles > 0, `grid shows Photos from "${shoot.name}" (${tiles} tiles, ${photos.length} in Library)`);
  const srcs = await p.locator('.tile img').evaluateAll((imgs) => imgs.map((i) => [i.getAttribute('src'), i.naturalWidth]));
  check(srcs.length > 0 && srcs.every(([s, w]) => s.startsWith('/previews/') && w > 0), 'tiles load engine preview JPEGs');

  // Re-read: the Queue worker may have culled since `photos` was read (an app-imported
  // Library has a cull queued), and ⌘Z must put back what was there just before the key.
  const first = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === photos[0].id);
  const want = first.verdict === 'pick' ? 'reject' : 'pick';
  await p.locator(`[data-testid="tile-${first.id}"]`).click();
  await p.keyboard.press(want === 'pick' ? 'p' : 'x');
  await p.waitForTimeout(500);
  const after = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === first.id);
  check(after.verdict === want && after.verdict_by === 'person', `pressing ${want === 'pick' ? 'P' : 'X'} wrote ${want} to the Library (got ${after.verdict} by ${after.verdict_by})`);
  await p.screenshot({ path: `${out}live-flagged.png` });

  // The Library is the only source of truth: ⌘Z is the engine's undo_batch, ⇧⌘Z undoes the Undo.
  const state = async (id) => (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === id);
  await p.keyboard.press('ControlOrMeta+z');
  await p.waitForTimeout(500);
  const unflagged = await state(first.id);
  check(unflagged.verdict === first.verdict && unflagged.verdict_by === first.verdict_by, `⌘Z undid the flag in the Library (got ${unflagged.verdict} by ${unflagged.verdict_by})`);
  await p.keyboard.press('ControlOrMeta+Shift+z');
  await p.waitForTimeout(500);
  const redone = await state(first.id);
  check(redone.verdict === want && redone.verdict_by === 'person', `⇧⌘Z redid it (got ${redone.verdict} by ${redone.verdict_by})`);
  await p.locator(`[data-testid="tile-${first.id}"]`).click(); // P moved on to the next Photo
  await p.keyboard.press('3');
  await p.waitForTimeout(500);
  const starred = await state(first.id);
  check(starred.stars === 3 && starred.stars_by === 'person', `pressing 3 wrote three stars to the Library (got ${starred.stars} by ${starred.stars_by})`);
  // Edit › Undo in the app's menu bar takes the same path as ⌘Z (src/ui/editMenu.ts).
  await p.evaluate(() => window.dispatchEvent(new CustomEvent('argus:edit-menu', { detail: 'undo' })));
  await p.waitForTimeout(500);
  check((await state(first.id)).stars === first.stars, 'Edit › Undo put the stars back');
  const shown = await p.locator(`[data-testid="tile-${first.id}"]`).getAttribute('data-verdict');
  check(shown === want, `the tile shows what the Library has (${shown})`);

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
    await p.waitForTimeout(600);
    const kept = (await api(`/api/photos?shoot_id=${shoot.id}`)).filter((x) => stack.some((y) => y.id === x.id));
    const best = kept.find((x) => x.id === stack[0].id);
    check(best.verdict === 'pick' && best.verdict_by === 'person' && kept.filter((x) => x.verdict === 'pick').length === 1, `Enter in the Stacks view kept the best frame of the burst (got ${kept.map((x) => `${x.verdict}/${x.verdict_by}`)})`);
    await p.keyboard.press('ControlOrMeta+z');
    await p.waitForTimeout(500);
    check((await state(stack[0].id)).verdict_by === 'ai', '⌘Z undid keeping it');
    await p.keyboard.press('Escape');
    await p.waitForSelector('.tile');
  }

  // Export the Picks to a folder chosen in the dialog (a prompt in a browser), with an
  // XMP-free JPEG preset, and check the files land named by Shoot.
  const exportDir = mkdtempSync(join(tmpdir(), 'argus-export-'));
  // The AI never picks (cautious cull), so the person picks the best frame for the rest.
  await api('/api/flag', { method: 'POST', body: JSON.stringify({ ids: [culled[0].id], verdict: 'pick', author: 'person' }) });
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

  // The Agent edits the Photo open in Edit, as one undoable Version.
  const recipeOf = async (id) => (await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: id }) }))?.recipe?.global ?? {};
  const target = photos[photos.length - 1].id;
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
  check(versions.length === 1 && versions[0].author === 'ai', `History shows the Agent's Version and nothing else (${versions.map((v) => v.label).join(', ')})`);
  await p.screenshot({ path: `${out}live-agent-edit.png` });
  await p.locator('.agent').getByRole('button', { name: 'Undo', exact: true }).click();
  await p.waitForTimeout(1000);
  const undone = await recipeOf(target);
  check(!undone.temp && !undone.contrast, `Undo in the chat put the Recipe back (${JSON.stringify(undone)})`);
  await p.screenshot({ path: `${out}live-agent-undo.png` });

  // A slider drag is one engine batch: ⌘Z is undo_batch on it, ⇧⌘Z undoes the Undo.
  const exposure = async () => (await recipeOf(target)).exposure ?? 0;
  await p.getByRole('tab', { name: 'Edit', exact: true }).click(); // History is open
  const thumb = p.getByRole('slider', { name: 'Exposure', exact: true });
  const nudge = async (n) => {
    await thumb.focus();
    for (let i = 0; i < n; i++) await p.keyboard.press('ArrowRight');
    await p.waitForTimeout(1700); // the drag rests, then saves
  };
  const versionsBefore = (await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) })).length;
  await nudge(4);
  const dragged = await exposure();
  await nudge(2);
  const second = await exposure();
  const drags = (await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) })).slice(versionsBefore);
  check(dragged > 0 && second > dragged && drags.length === 2 && drags.every((v) => v.author === 'person' && /^Exposure /.test(v.label ?? '')), `two Exposure drags saved as two Versions (${drags.map((v) => v.label).join(', ')})`);
  await p.locator('.stage').click();
  await p.keyboard.press('ControlOrMeta+z');
  await p.waitForTimeout(800);
  check((await exposure()) === dragged, `⌘Z undid the last drag in the Library (exposure ${await exposure()}, want ${dragged})`);
  await p.keyboard.press('ControlOrMeta+z');
  await p.waitForTimeout(800);
  check((await exposure()) === 0, `⌘Z again undid the first drag (exposure ${await exposure()})`);
  await p.keyboard.press('ControlOrMeta+Shift+z');
  await p.waitForTimeout(800);
  check((await exposure()) === dragged, `⇧⌘Z redid it (exposure ${await exposure()})`);
  const label = await p.locator('.slider').filter({ has: p.locator('.slider-label', { hasText: /^Exposure$/ }) }).textContent();
  check(label?.includes(dragged.toFixed(2)), `the slider shows what the Library has (${label})`);

  // Suggestions: R queues new takes, the bridge's Queue worker asks the model, and the
  // takes show up under the photo; accepting one saves it as a Version.
  await p.locator('.stage').click();
  const suggestBefore = suggestCalls;
  await p.keyboard.press('r');
  await p.locator('.take-name', { hasText: 'Punchy' }).waitFor({ timeout: 30000 });
  // The takes may already show from the start-up Suggestions; wait for R's own requests.
  for (let i = 0; i < 60 && suggestCalls - suggestBefore < 2; i++) await p.waitForTimeout(500);
  await p.waitForTimeout(1000);
  // One proposal plus one Review round (pinned above, the Full default).
  check(suggestCalls - suggestBefore === 2, `the Queue asked the model for takes and one Review round (${suggestCalls - suggestBefore})`);
  await p.keyboard.press('Tab');
  await p.keyboard.press('Tab');
  await p.getByTestId('previewing').getByText(/Punchy/).waitFor({ timeout: 5000 });
  await p.waitForTimeout(800); // the take's render
  await p.screenshot({ path: `${out}live-suggestions.png` });
  await p.keyboard.press('Enter');
  await p.waitForTimeout(1200);
  const accepted = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
  check(accepted?.recipe?.global?.exposure === 0.7 && accepted?.recipe?.crop?.w === 0.8, `accepting a take saved it with its crop (${JSON.stringify(accepted?.recipe)})`);
  const history = await api('/api/list_versions', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
  check(history.at(-1)?.label === 'Suggestion: Punchy', `the take is a Version of its own (${history.at(-1)?.label})`);

  // Edit: a Sky Mask is rendered by the engine and its real Mask is tinted on top.
  await p.goto(`http://localhost:5198/?library=live#/edit/${target}`);
  await p.waitForSelector('.stage .photo img');
  await p.waitForTimeout(1500);
  const before = await p.locator('.stage .photo img').first().getAttribute('src');
  // Auto: the engine measures the Photo and saves Lightroom's Auto sliders as one batch.
  await p.getByRole('tab', { name: 'Edit', exact: true }).click(); // History may still be open
  await p.getByTestId('auto').click();
  await p.waitForFunction((b) => document.querySelector('.stage .photo img')?.getAttribute('src') !== b, before, { timeout: 20000 });
  const autoed = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
  const autoKeys = ['exposure', 'contrast', 'highlights', 'shadows', 'whites', 'blacks', 'temp', 'tint', 'vibrance'].filter((k) => autoed?.recipe?.global?.[k]);
  check(autoKeys.length >= 3 && autoed?.recipe?.crop?.w === 0.8, `Auto set ${autoKeys.join(', ')} and kept the crop`);
  const afterAuto = await p.locator('.stage .photo img').first().getAttribute('src');
  // A preset is the person's own edit: its sliders reach the Library, the rest is kept.
  await p.getByRole('tab', { name: 'Presets' }).click();
  await p.getByTestId('preset-warm-film').click();
  await p.waitForTimeout(1500);
  const preset = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
  check(preset?.recipe?.global?.temp === 16 && preset?.recipe?.crop?.w === 0.8, `the Warm film preset was saved (temp ${preset?.recipe?.global?.temp})`);
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
  // A Recommended Look and a one-click fix from the Edit tool reach the Library.
  await p.getByRole('tab', { name: 'Edit', exact: true }).click();
  await p.getByTestId('rec-moody').click();
  await p.getByTestId('fix-shadows').click();
  await p.waitForTimeout(1500);
  const rec = await api('/api/read_recipe', { method: 'POST', body: JSON.stringify({ photo_id: target }) });
  check(rec?.recipe?.global?.contrast === 22 && rec?.recipe?.global?.shadows === 30, `Recommended Moody and Lift shadows were saved (contrast ${rec?.recipe?.global?.contrast}, shadows ${rec?.recipe?.global?.shadows})`);

  // The Look screen runs on the Library's Looks: a slider drag and Remove Look are batches
  // of the person's own, and ⌘Z / ⇧⌘Z are undo_batch.
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
  await p.waitForTimeout(1700);
  check((await lookContrast()) === 5, `a Look slider drag reached the Library (contrast ${await lookContrast()})`);
  await p.locator(`[data-testid="batch-${pick.id}"]`).click();
  await p.keyboard.press('ControlOrMeta+z');
  await p.waitForTimeout(800);
  check((await lookContrast()) === 0, `⌘Z undid the Look drag in the Library (contrast ${await lookContrast()})`);
  await p.keyboard.press('ControlOrMeta+Shift+z');
  await p.waitForTimeout(800);
  check((await lookContrast()) === 5, `⇧⌘Z redid it (contrast ${await lookContrast()})`);
  await p.getByRole('button', { name: 'Remove Look' }).click();
  await p.waitForTimeout(800);
  check((await lookOf()) === null, `Remove Look took the Pick out from under it (${await lookOf()})`);
  await p.locator(`[data-testid="batch-${pick.id}"]`).click();
  await p.keyboard.press('ControlOrMeta+z');
  await p.getByRole('heading', { name: 'Look: Test look' }).waitFor({ timeout: 5000 }).catch(() => {});
  check((await lookOf()) === made.look.id, `⌘Z put the Look back (${await lookOf()})`);
  check(await p.getByRole('heading', { name: 'Look: Test look' }).isVisible(), 'the Look screen shows it again');
  await p.screenshot({ path: `${out}live-look.png` });
  // Renaming the Shoot from the sidebar goes through rename_shoot: the stored name follows
  // the naming rules and every Photo still finds its original.
  await p.goto('http://localhost:5198/?library=live#/cull');
  await p.waitForSelector('.tile');
  const row = p.getByTestId(`shoot-${shoot.id}`);
  await row.dblclick();
  await p.keyboard.press('ControlOrMeta+a');
  await p.keyboard.type('2026-10-10 Boat tour');
  await p.keyboard.press('Enter');
  await p.waitForTimeout(500);
  const renamed = (await api('/api/list_shoots')).find((s) => s.id === shoot.id);
  check(renamed.name === '2026-10-10_Boat-tour', `renaming in the sidebar stored the Shoot as ${renamed.name}`);
  check((await row.textContent()).includes('2026-10-10_Boat-tour'), 'the sidebar shows the new name');
  const missing = await api('/api/find_missing_originals', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' });
  check(Array.isArray(missing.missing ?? missing) && (missing.missing ?? missing).length === 0, 'no original went missing after the rename');

  // Remove from Library: ⌫ asks, Remove takes the Photo out of the Library (originals stay),
  // ⌘Z right after puts it back with its flag.
  const inShoot = await api(`/api/photos?shoot_id=${shoot.id}`);
  const victim = inShoot[0];
  await p.locator(`[data-testid="tile-${victim.id}"]`).click();
  await p.keyboard.press('Backspace');
  await p.getByTestId('remove-dialog').waitFor();
  await p.screenshot({ path: `${out}live-remove-ask.png` });
  await p.getByTestId('remove-confirm').click();
  await p.waitForTimeout(600);
  const without = await api(`/api/photos?shoot_id=${shoot.id}`);
  check(without.length === inShoot.length - 1 && !without.some((x) => x.id === victim.id), `Remove took ${victim.file_name} out of the Library (${without.length} left)`);
  check(existsSync(victim.path), 'its original is still on disk');
  check((await p.locator(`[data-testid="tile-${victim.id}"]`).count()) === 0, 'the grid dropped it');
  await p.screenshot({ path: `${out}live-removed.png` });
  await p.keyboard.press('ControlOrMeta+z');
  await p.waitForTimeout(800);
  const back = (await api(`/api/photos?shoot_id=${shoot.id}`)).find((x) => x.id === victim.id);
  check(back && back.verdict === victim.verdict, `⌘Z put it back with its Verdict (${back?.verdict})`);
  check((await p.locator(`[data-testid="tile-${victim.id}"]`).count()) === 1, 'the grid shows it again');

  // Library-wide views from the sidebar: every Photo, then every Pick.
  const everything = await api('/api/photos');
  await p.getByTestId('nav-all').click();
  await p.waitForSelector('.tile');
  await p.waitForTimeout(600);
  check((await p.locator('h1').first().textContent()) === 'All photos', 'All photos opens a Library-wide grid');
  check((await p.locator('[data-testid="ai-cull"]').count()) === 0, 'the Library-wide grid hides the Shoot-only AI cull');
  await p.getByTestId('nav-picks').click();
  await p.waitForSelector('[data-testid="nav-picks"].active');
  await p.waitForTimeout(600);
  const picksShown = Number(await p.getByTestId('nav-picks').locator('.count').textContent());
  check(picksShown === everything.filter((x) => x.verdict === 'pick').length, `Picks shows every Pick in the Library (${picksShown})`);
  await p.screenshot({ path: `${out}live-picks.png` });
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
