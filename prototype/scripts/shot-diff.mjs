// Screenshot baselines: compares the dummy-data shots (`npm run shots`) against the last
// blessed set and fails on a screen that changed by more than a sliver, so a CSS edit can't
// quietly move another screen. Writes a red-on-grey diff for each change to out/shots/diff/.
//
//   node scripts/shot-diff.mjs                 compare out/shots/ with the baselines
//   node scripts/shot-diff.mjs --bless [names] accept the current shots (all, or names containing these words)
//   node scripts/shot-diff.mjs --from DIR      compare DIR instead of out/shots/ (e.g. two runs, to find flaky shots)
//
// Baselines are Linux Chromium renders (fonts differ on the Mac), 17 MB a set, so they live in
// the project's shared folder rather than git: ARGUS_SHOT_BASELINES, default
// /mnt/project-files/baselines/shots. `blessed.json` there says which commit each came from.
// Screens with live clocks or animations are in UNSTABLE and only checked for existing.

import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

// Playwright's own comparator (pixelmatch with anti-aliasing detection), so no new dependency.
// Not in the package's exports, so load it by path from where playwright-core is installed.
const require = createRequire(import.meta.url);
const core = dirname(require.resolve('playwright-core'));
const { getComparator } = require(join(core, 'lib/server/utils/comparators.js'));
const compare = getComparator('image/png');

// Shots that differ between two runs of the same commit (found with --from on two runs).
const UNSTABLE = [];
// A screen may move this share of its pixels (anti-aliasing, a caret) before it counts as changed.
const MAX_RATIO = 0.002;

const args = process.argv.slice(2);
const bless = args.includes('--bless');
const fromAt = args.indexOf('--from');
const shots = fromAt >= 0 ? args[fromAt + 1] : new URL('../out/shots/', import.meta.url).pathname;
const names = args.filter((a, i) => !a.startsWith('--') && i !== fromAt + 1);
const base = process.env.ARGUS_SHOT_BASELINES ?? '/mnt/project-files/baselines/shots';
const diffDir = new URL('../out/shots/diff/', import.meta.url).pathname;

// Only the dummy-data shots: live-* and mac-* run on real engines and real timing.
const lastRun = join(shots, 'last-run.json');
const run = existsSync(lastRun) && fromAt < 0 ? JSON.parse(readFileSync(lastRun, 'utf8')) : null;
const shot = run ? new Set(run.shots.map((n) => `${n}.png`)) : null;
// A filtered run (`shots.mjs cull`) only shot some screens; the rest aren't missing.
const partial = names.length > 0 || (run?.filters?.length ?? 0) > 0;
const current = readdirSync(shots).filter((f) => f.endsWith('.png') && !/^(live|mac)-/.test(f) && (!shot || shot.has(f)));
const wanted = (f) => !names.length || names.some((n) => f.includes(n));

if (bless) {
  mkdirSync(base, { recursive: true });
  const ledgerPath = join(base, 'blessed.json');
  const ledger = existsSync(ledgerPath) ? JSON.parse(readFileSync(ledgerPath, 'utf8')) : {};
  const commit = execSync('git rev-parse --short HEAD').toString().trim();
  const blessed = current.filter(wanted);
  for (const f of blessed) {
    copyFileSync(join(shots, f), join(base, f));
    ledger[f] = { commit, at: new Date().toISOString() };
  }
  writeFileSync(ledgerPath, JSON.stringify(ledger, null, 1) + '\n');
  console.log(`shot-diff: blessed ${blessed.length} shot(s) at ${commit} into ${base}`);
  process.exit(0);
}

if (!existsSync(base)) {
  console.log(`shot-diff: no baselines in ${base}; run with --bless on a known-good main first`);
  process.exit(0);
}
mkdirSync(diffDir, { recursive: true });
const baselines = readdirSync(base).filter((f) => f.endsWith('.png') && !/^(live|mac)-/.test(f) && wanted(f));
const changed = [];
const missing = [];
let same = 0;
for (const f of baselines) {
  if (!current.includes(f)) {
    if (!partial) missing.push(f);
    continue;
  }
  if (UNSTABLE.includes(f.replace(/\.png$/, ''))) {
    same++;
    continue;
  }
  const r = compare(readFileSync(join(shots, f)), readFileSync(join(base, f)), { maxDiffPixelRatio: MAX_RATIO, threshold: 0.2 });
  if (!r) {
    same++;
    continue;
  }
  if (r.diff) writeFileSync(join(diffDir, f), r.diff);
  changed.push(`${f}: ${r.errorMessage?.split('\n')[0] ?? 'differs'}`);
}
const fresh = current.filter((f) => wanted(f) && !baselines.includes(f));
console.log(`shot-diff: ${same} same, ${changed.length} changed, ${missing.length} missing, ${fresh.length} new (bless them to start checking)`);
for (const c of changed) console.log(`changed  ${c}`);
for (const m of missing) console.log(`missing  ${m} (not shot this run; rename or bless)`);
if (changed.length) console.log(`\nLook at out/shots/<name>.png and the diff in out/shots/diff/. Intended? node scripts/shot-diff.mjs --bless <names>`);
process.exit(changed.length ? 1 : 0);
