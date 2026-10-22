// Presets parity: src/domain/presets.ts re-implements engine/src/presets.rs (a Preset applied at
// an Amount, then taken off), and this checks they agree. Every built-in Preset goes on a neutral
// Recipe, a busy one (Auto-like values) and a busy one with Moody already on, at Amounts 50, 100
// and 150%, through the engine's `apply_preset` tool and through presets.ts, then comes off again
// (`remove`). Any difference in `global`, the Preset's Masks or `recipe.presets` is printed and
// the exit code is 1. About a minute (one CLI call per step); run it after touching either file.
//
//   node scripts/preset-parity.mjs [--only natural,moody]
//   ARGUS_BIN=../target/fast/argus   the engine binary (default shown; `cargo build --profile fast -p argus-cli`)

import { build } from 'esbuild';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const bin = resolve(here, '..', process.env.ARGUS_BIN ?? '../target/fast/argus');
if (!existsSync(bin)) {
  console.error(`no engine binary at ${bin}: build it with cargo build --profile fast -p argus-cli`);
  process.exit(2);
}
const only = process.argv.includes('--only') ? process.argv[process.argv.indexOf('--only') + 1].split(',') : null;

const tmp = mkdtempSync(join(tmpdir(), 'preset-parity-'));
process.on('exit', () => rmSync(tmp, { recursive: true, force: true }));

// presets.ts as plain JS (it imports the engine's builtin.json, which esbuild inlines).
const bundle = join(tmp, 'presets.mjs');
await build({ entryPoints: [resolve(here, '../src/domain/presets.ts')], bundle: true, format: 'esm', platform: 'node', outfile: bundle, logLevel: 'error' });
const { PRESETS, applyPreset, removePreset } = await import(pathToFileURL(bundle).href);

// A tiny grey JPEG is all a Library needs to hold a Recipe.
const card = join(tmp, 'card');
mkdirSync(card);
const JPEG =
  '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDABALDA4MChAODQ4SERATGCgaGBYWGDEjJR0oOjM9PDkzODdASFxOQERXRTc4UG1RV19iZ2hnPk1xeXBkeFxlZ2P/2wBDARESEhgVGC8aGi9jQjhCY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2NjY2P/wAARCAAIAAgDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwAooooA/9k=';
writeFileSync(join(card, 'pixel.jpg'), Buffer.from(JPEG, 'base64'));
const lib = join(tmp, 'lib');
execFileSync(bin, ['import', card, '--library', lib, '--camera-previews'], { stdio: 'ignore' });
const tool = (name, args) => JSON.parse(execFileSync(bin, ['tool', name, JSON.stringify(args), '--library', lib], { encoding: 'utf8' }));

const recipe = (global) => ({ schema: 1, global, local: [], look: null, correction: { exposure: 0, wbK: 0, face: 0 } });
const BUSY = { exposure: 0.4, contrast: 12, highlights: -35, shadows: 30, saturation: 10, temp: -6, vignetteMidpoint: 40, hsl: { blue: { sat: -10 } } };
// `first` is a Preset already on, which a Look-style Preset replaces and an Adaptive one joins.
const BASES = { neutral: { global: {} }, busy: { global: BUSY }, 'busy after moody': { global: BUSY, first: 'moody' } };

// The engine's Recipe leaves out a slider at 0 (and a group left empty) when it saves; the UI
// keeps it. Same edit, so both drop them before comparing.
function dropZeros(v) {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return v;
  const out = {};
  for (const [k, x] of Object.entries(v)) {
    const y = dropZeros(x);
    if (y !== 0 && !(y && typeof y === 'object' && !Array.isArray(y) && !Object.keys(y).length)) out[k] = y;
  }
  return out;
}

// The parts both sides own: what the Preset changed, its Masks and its record in recipe.presets.
const comparable = (r) => ({
  global: dropZeros(r.global ?? {}),
  local: (r.local ?? []).map((l) => ({ id: l.id, mask: l.mask, adjust: dropZeros(l.adjust) })),
  presets: r.presets ?? [],
});

function diff(a, b, path = '', out = []) {
  if (typeof a === 'number' && typeof b === 'number') {
    if (Math.abs(a - b) > 1e-6) out.push(`${path}: engine ${a}, ui ${b}`);
  } else if (a && b && typeof a === 'object' && typeof b === 'object') {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) diff(a[k], b[k], path ? `${path}.${k}` : k, out);
  } else if (JSON.stringify(a) !== JSON.stringify(b)) out.push(`${path}: engine ${JSON.stringify(a)}, ui ${JSON.stringify(b)}`);
  return out;
}

let cases = 0;
let failed = 0;
const presets = PRESETS.filter((p) => !only || only.includes(p.id));
for (const [i, p] of presets.entries()) {
  for (const [baseName, { global, first }] of Object.entries(BASES)) {
    for (const amount of [0.5, 1, 1.5]) {
      let base = recipe(global);
      tool('write_recipe', { photo_ids: [1], recipe: base });
      if (first) {
        tool('apply_preset', { photo_ids: [1], preset: first });
        base = applyPreset(base, PRESETS.find((x) => x.id === first), 1);
      }
      tool('apply_preset', { photo_ids: [1], preset: p.id, amount: amount * 100 });
      const engineOn = tool('read_recipe', { photo_id: 1 }).recipe;
      const uiOn = applyPreset(base, p, amount);
      tool('apply_preset', { photo_ids: [1], preset: p.id, remove: true });
      const engineOff = tool('read_recipe', { photo_id: 1 }).recipe;
      const uiOff = removePreset(uiOn, p);
      const problems = [
        ...diff(comparable(engineOn), comparable(uiOn)).map((d) => `on: ${d}`),
        ...diff(comparable(engineOff), comparable(uiOff)).map((d) => `off: ${d}`),
      ];
      cases++;
      if (problems.length) {
        failed++;
        console.log(`✗ ${p.id} on ${baseName} at ${amount * 100}%\n    ${problems.join('\n    ')}`);
      }
    }
  }
  console.log(`::progress ${i + 1}/${presets.length} ${p.id}`);
}
console.log(failed ? `preset-parity: ${failed} of ${cases} cases differ` : `preset-parity: ok, ${cases} cases agree`);
process.exitCode = failed ? 1 : 0;
