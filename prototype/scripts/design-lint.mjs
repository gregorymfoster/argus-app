// Design lint (docs/design.md, "Checking a UI change"): counts the things the design language rules out,
// per file, and fails when any count grows past design-lint-baseline.json. It's a ratchet, so old code
// keeps its count until someone cleans it up, and new code can't add more.
//
//   node scripts/design-lint.mjs            check against the baseline
//   node scripts/design-lint.mjs --update   write the current counts as the new baseline (after a cleanup)
//   node scripts/design-lint.mjs --list     print every hit with its line
import { readFileSync, readdirSync, writeFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const root = new URL('..', import.meta.url).pathname;
const src = join(root, 'src');
const baselinePath = join(root, 'scripts/design-lint-baseline.json');

// The scales in docs/design.md. 2px is for thin bars and dots, 0 resets.
const RADII = new Set([0, 2, 4, 5, 6, 7, 8, 10, 999]);
const SIZES = new Set([10, 10.5, 11, 11.5, 12, 12.5, 13, 13.5, 15]);

const RULES = {
  color: 'color literal outside the :root and @theme token blocks (use a token)',
  radius: 'radius off the scale 2, 4-8, 10, 999',
  'font-size': 'font size off the scale 10-13.5, 15',
  kbd: 'bare <kbd> (use <Kbd> or <Kbd quiet> from ui/kit)',
  spinner: 'spinner (use ProgressBar with a count and an ETA)',
  'em-dash': 'em-dash in UI text',
};

// Files that draw fake photos for the dummy Library, not UI, so their colors aren't chrome.
const PAINTERS = new Set(['src/data/dummy.ts', 'src/data/painter.ts', 'src/domain/render.ts']);
// Primitives allowed to write a bare <kbd>: the kit's Kbd itself and the ⌘K palette's footer.
const KBD_OK = new Set(['src/ui/kit.tsx', 'src/components/ui/command.tsx']);

function walk(dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return n === 'fonts' ? [] : walk(p);
    return /\.(css|tsx|ts)$/.test(n) && !n.endsWith('.d.ts') && !n.includes('.test.') ? [p] : [];
  });
}

// Blank out top-level `:root ... { }` and `@theme ... { }` blocks (the token definitions), keeping line numbers.
function stripTokenBlocks(css) {
  let out = '';
  let i = 0;
  while (i < css.length) {
    const m = /(^|\n)\s*(:root[^{,]*|@theme[^{]*)\{/g;
    m.lastIndex = i;
    const hit = m.exec(css);
    if (!hit) { out += css.slice(i); break; }
    const open = hit.index + hit[0].length - 1;
    let depth = 0, j = open;
    for (; j < css.length; j++) {
      if (css[j] === '{') depth++;
      else if (css[j] === '}' && --depth === 0) break;
    }
    out += css.slice(i, open) + css.slice(open, j + 1).replace(/[^\n]/g, ' ');
    i = j + 1;
  }
  return out;
}

function stripComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' ')).replace(/(^|[^:])\/\/.*$/gm, (c, a) => a + ' '.repeat(c.length - a.length));
}

function scan(file) {
  const rel = relative(root, file);
  const isCss = file.endsWith('.css');
  let text = stripComments(readFileSync(file, 'utf8'));
  if (isCss) text = stripTokenBlocks(text);
  const hits = [];
  const lines = text.split('\n');
  lines.forEach((line, n) => {
    const at = (rule, what) => hits.push({ rule, line: n + 1, what });
    for (const m of line.matchAll(/#[0-9a-fA-F]{3,8}\b|rgba?\(\s*\d/g)) {
      // Hex in TSX only counts inside a style or class string, not ids like href="#/cull".
      if (!isCss && m[0].startsWith('#') && /[#/]/.test(line[m.index - 1] ?? '')) continue;
      if (!isCss && m[0].startsWith('#') && !/['"`\[:(\s]/.test(line[m.index - 1] ?? '')) continue;
      at('color', m[0]);
    }
    for (const m of line.matchAll(/border(?:-[a-z]+)*-radius:\s*([^;}]+)/g))
      for (const v of m[1].matchAll(/(\d+(?:\.\d+)?)px/g)) if (!RADII.has(Number(v[1]))) at('radius', v[0]);
    for (const m of line.matchAll(/rounded(?:-[a-z]+)?-\[(\d+(?:\.\d+)?)px\]/g)) if (!RADII.has(Number(m[1]))) at('radius', m[0]);
    for (const m of line.matchAll(/font-size:\s*(\d+(?:\.\d+)?)px/g)) if (!SIZES.has(Number(m[1]))) at('font-size', m[0]);
    for (const m of line.matchAll(/text-\[(\d+(?:\.\d+)?)px\]/g)) if (!SIZES.has(Number(m[1]))) at('font-size', m[0]);
    for (const m of line.matchAll(/fontSize:\s*(\d+(?:\.\d+)?)\b/g)) if (!SIZES.has(Number(m[1]))) at('font-size', m[0]);
    if (!isCss) {
      if (/<kbd[\s>]/.test(line) && !KBD_OK.has(rel)) at('kbd', '<kbd>');
      if (/\banimate-spin\b|\bLoader2\b|\bclassName=["'][^"']*\bspinner\b/.test(line)) at('spinner', 'spinner');
      if (line.includes('—')) at('em-dash', '—');
    }
  });
  return { rel, hits };
}

const results = walk(src).filter((f) => !PAINTERS.has(relative(root, f))).map(scan).filter((r) => r.hits.length);
const counts = {};
for (const { rel, hits } of results) {
  for (const h of hits) {
    counts[rel] ??= {};
    counts[rel][h.rule] = (counts[rel][h.rule] ?? 0) + 1;
  }
}

const args = process.argv.slice(2);
if (args.includes('--list')) {
  for (const { rel, hits } of results) for (const h of hits) console.log(`${rel}:${h.line}  ${h.rule}  ${h.what}`);
}
if (args.includes('--update')) {
  const sorted = Object.fromEntries(Object.keys(counts).sort().map((k) => [k, Object.fromEntries(Object.entries(counts[k]).sort())]));
  writeFileSync(baselinePath, JSON.stringify(sorted, null, 2) + '\n');
  const total = Object.values(counts).reduce((s, c) => s + Object.values(c).reduce((a, b) => a + b, 0), 0);
  console.log(`design-lint: baseline written (${total} known hits in ${Object.keys(counts).length} files)`);
  process.exit(0);
}

let baseline = {};
try { baseline = JSON.parse(readFileSync(baselinePath, 'utf8')); } catch {}
const worse = [];
let better = 0;
for (const [file, byRule] of Object.entries(counts)) {
  for (const [rule, n] of Object.entries(byRule)) {
    const was = baseline[file]?.[rule] ?? 0;
    if (n > was) worse.push({ file, rule, n, was });
  }
}
for (const [file, byRule] of Object.entries(baseline))
  for (const [rule, was] of Object.entries(byRule)) if ((counts[file]?.[rule] ?? 0) < was) better++;

if (worse.length) {
  console.error('design-lint: new design-language violations (docs/design.md):');
  for (const w of worse) {
    console.error(`  ${w.file}: ${w.n} ${w.rule} (baseline ${w.was}), ${RULES[w.rule]}`);
    const hits = results.find((r) => r.rel === w.file).hits.filter((h) => h.rule === w.rule);
    for (const h of hits.slice(0, 6)) console.error(`    line ${h.line}: ${h.what}`);
    if (hits.length > 6) console.error(`    and ${hits.length - 6} more (--list shows all)`);
  }
  console.error('Fix them, or if a value is right and the scale is wrong, change docs/design.md and the scale here.');
  process.exit(1);
}
console.log(`design-lint: ok${better ? ` (${better} counts went down: run with --update to lock them in)` : ''}`);
