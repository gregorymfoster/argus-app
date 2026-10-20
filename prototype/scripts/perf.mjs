// How light the app is while nothing heavy runs: the built UI against a real Library
// through `argus serve`, in Chromium. Measures what the person pays for an open window:
//
//   startup     page load to the first grid thumbnail on screen, JS and CSS shipped
//   idle        engine calls, bytes, React commits and CPU (bridge and page) per minute
//               with the grid open and nothing happening
//   hidden      the same with the window hidden (minimised or behind another app)
//   scroll      memory after scrolling the whole grid: JS heap, page RSS, image bytes
//
// Works on a copy of the Library. Prints one JSON report and writes it to out/perf/.
//
//   npm run build && node scripts/perf.mjs [library-dir] [--idle 60] [--label name] [--profile]
//   (`just perf` does both.) Without a Library it imports the sample RAWs and copies their
//   rows up to --photos 1000, each with its own preview file (hard links), so the grid
//   loads 1,000 different images.
//   ARGUS_BIN=../target/fast/argus    the bridge binary (default shown)
//   --profile                         also the page's top functions while scrolling
//
// Linux only (reads /proc): run it in a cloud session, not on the Mac.
//
// Compare two reports with `node scripts/perf.mjs --diff a.json b.json`.

import { preview } from 'vite';
import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { cpSync, existsSync, linkSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const argv = process.argv.slice(2);
const opt = (name, d) => {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv.splice(i, 2)[1] : d;
};

if (argv[0] === '--diff') {
  const [a, b] = argv.slice(1).map((f) => JSON.parse(readFileSync(f, 'utf8')));
  const flat = (o, pre = '') => Object.entries(o).flatMap(([k, v]) => (v && typeof v === 'object' ? flat(v, `${pre}${k}.`) : typeof v === 'number' ? [[`${pre}${k}`, v]] : []));
  const fb = new Map(flat(b));
  for (const [k, va] of flat(a)) {
    const vb = fb.get(k);
    if (vb == null) continue;
    const pct = va ? Math.round(((vb - va) / va) * 100) : 0;
    console.log(`${k.padEnd(40)} ${String(va).padStart(12)} -> ${String(vb).padStart(12)}  ${pct > 0 ? '+' : ''}${pct}%`);
  }
  process.exit(0);
}

const photos = Number(opt('photos', '1000'));
const idleS = Number(opt('idle', '60'));
const profile = argv.includes('--profile') && argv.splice(argv.indexOf('--profile'), 1);
const label = opt('label', new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-'));
const bin = process.env.ARGUS_BIN ?? new URL('../../target/fast/argus', import.meta.url).pathname;
const dist = new URL('../dist/', import.meta.url).pathname;
if (!existsSync(join(dist, 'index.html'))) {
  console.error('build the UI first: npm run build');
  process.exit(2);
}
const library = join(mkdtempSync(join(tmpdir(), 'argus-perf-')), 'lib');
const source = argv[0] ?? sampleLibrary(library, photos);
if (argv[0]) cpSync(source, library, { recursive: true });

// The sample RAWs imported with camera previews, their rows copied up to `n` Photos.
function sampleLibrary(dir, n) {
  const raws = ['raw', 'raw-a7rv'].map((d) => new URL(`../../fixtures/${d}`, import.meta.url).pathname).filter((d) => existsSync(d) && readdirSync(d).some((f) => /\.arw$/i.test(f)));
  if (!raws.length) throw new Error('no sample RAWs in fixtures/raw or fixtures/raw-a7rv');
  for (const r of raws) execFileSync(bin, ['import', r, '--library', dir, '--camera-previews'], { stdio: 'ignore' });
  const { DatabaseSync } = createRequire(import.meta.url)('node:sqlite');
  const db = new DatabaseSync(join(dir, 'library.db'));
  db.exec('UPDATE photos SET shoot_id = 1');
  const rows = db.prepare('SELECT * FROM photos').all();
  const cols = Object.keys(rows[0]).filter((c) => c !== 'id');
  const insert = db.prepare(`INSERT INTO photos (${cols.join(',')}) VALUES (${cols.map(() => '?').join(',')})`);
  for (let i = rows.length; i < n; i++) {
    const r = { ...rows[i % rows.length] };
    r.hash = createHash('sha256').update(`copy ${i}`).digest('hex');
    r.file_name = `DSC${String(10000 + i)}.ARW`;
    const preview = `previews/${r.hash.slice(0, 2)}/${r.hash}.camera.jpg`;
    mkdirSync(join(dir, 'previews', r.hash.slice(0, 2)), { recursive: true });
    linkSync(join(dir, r.preview), join(dir, preview));
    r.preview = preview;
    insert.run(...cols.map((c) => r[c]));
  }
  db.exec('DELETE FROM shoots WHERE id NOT IN (SELECT shoot_id FROM photos)');
  db.close();
  return `sample RAWs x ${n}`;
}

const bridge = spawn(bin, ['serve', '--library', library, '--port', '7878'], {
  stdio: ['ignore', 'ignore', 'inherit'],
  // No model here: point the Providers at a closed port so nothing waits on one.
  env: { ...process.env, OLLAMA_HOST: '127.0.0.1:9', ARGUS_POWER: 'ac' },
});
for (let i = 0; ; i++) {
  try {
    await fetch('http://127.0.0.1:7878/api/list_shoots', { method: 'POST', body: '{}' });
    break;
  } catch (e) {
    if (i > 100) throw e;
    await new Promise((r) => setTimeout(r, 100));
  }
}

// CPU seconds and context switches (each one a wakeup) of a process and all its threads.
function procStat(pid) {
  let cpu = 0;
  let wakeups = 0;
  for (const t of readdirSync(`/proc/${pid}/task`)) {
    const stat = readFileSync(`/proc/${pid}/task/${t}/stat`, 'utf8');
    const f = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
    cpu += (Number(f[11]) + Number(f[12])) / 100;
    const st = readFileSync(`/proc/${pid}/task/${t}/status`, 'utf8');
    wakeups += Number(/voluntary_ctxt_switches:\s+(\d+)/.exec(st)[1]);
  }
  const rss = Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, 'utf8'))[1]) / 1024;
  return { cpu, wakeups, rss };
}
// Chromium's renderer processes (the page), by command line.
function renderers() {
  const out = execFileSync('ps', ['-eo', 'pid=,args='], { encoding: 'utf8' });
  return out
    .split('\n')
    .filter((l) => l.includes('--type=renderer') && !l.includes('--extension-process'))
    .map((l) => Number(l.trim().split(' ')[0]));
}
const pageStat = () => renderers().reduce((s, pid) => {
  try {
    const p = procStat(pid);
    return { cpu: s.cpu + p.cpu, wakeups: s.wakeups + p.wakeups, rss: s.rss + p.rss };
  } catch {
    return s;
  }
}, { cpu: 0, wakeups: 0, rss: 0 });
const r1 = (n) => Math.round(n * 10) / 10;

const server = await preview({ root: new URL('..', import.meta.url).pathname, preview: { port: 5199, strictPort: true }, logLevel: 'error' });
const browser = await chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
const report = { label, library: source };
const out = new URL(`../out/perf/`, import.meta.url).pathname;
try {
  const ctx = await browser.newContext({ viewport: { width: 1500, height: 900 } });
  // Count React commits through the DevTools hook React calls in production builds too.
  await ctx.addInitScript(() => {
    window.__commits = 0;
    window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
      supportsFiber: true,
      renderers: new Map(),
      inject() {
        return 1;
      },
      onCommitFiberRoot() {
        window.__commits++;
      },
      onCommitFiberUnmount() {},
      onPostCommitFiberRoot() {},
      checkDCE() {},
    };
    // Lets the run hide the page as a minimised window would.
    window.__hidden = false;
    Object.defineProperty(document, 'hidden', { get: () => window.__hidden });
    Object.defineProperty(document, 'visibilityState', { get: () => (window.__hidden ? 'hidden' : 'visible') });
  });
  const page = await ctx.newPage();
  const calls = new Map();
  let bytes = 0;
  let images = 0;
  let imageBytes = 0;
  let counting = false;
  page.on('response', async (res) => {
    const url = new URL(res.url());
    const size = Number(res.headers()['content-length'] ?? 0) || (await res.body().catch(() => Buffer.alloc(0))).length;
    if (url.pathname.startsWith('/api/')) {
      if (!counting) return;
      const c = url.pathname.slice(5);
      const e = calls.get(c) ?? { n: 0, bytes: 0 };
      e.n++;
      e.bytes += size;
      calls.set(c, e);
      bytes += size;
    } else if (/\.(jpe?g|webp|png)$/i.test(url.pathname)) {
      images++;
      imageBytes += size;
    }
  });

  // Startup.
  const t0 = Date.now();
  await page.goto('http://127.0.0.1:5199/?library=live#/cull');
  await page.waitForFunction(() => [...document.querySelectorAll('.layer img')].some((i) => i.complete && i.naturalWidth > 0), null, { timeout: 30000 });
  report.startup = { first_thumb_ms: Date.now() - t0 };
  const assets = readdirSync(join(dist, 'assets')).map((f) => ({ f, b: statSync(join(dist, 'assets', f)).size }));
  report.bundle = {
    js_kb: Math.round(assets.filter((a) => a.f.endsWith('.js')).reduce((s, a) => s + a.b, 0) / 1024),
    css_kb: Math.round(assets.filter((a) => a.f.endsWith('.css')).reduce((s, a) => s + a.b, 0) / 1024),
    fonts_kb: Math.round(assets.filter((a) => /\.(woff2?|ttf)$/.test(a.f)).reduce((s, a) => s + a.b, 0) / 1024),
  };
  await page.waitForTimeout(3000);
  report.page_rss_mb_start = Math.round(pageStat().rss);
  report.tiles_on_screen = await page.evaluate(() => document.querySelectorAll('.layer img').length);
  mkdirSync(out, { recursive: true });
  await page.screenshot({ path: join(out, `${label}-start.png`) });

  // Idle, visible then hidden.
  const measure = async (name) => {
    calls.clear();
    bytes = 0;
    const commits0 = await page.evaluate(() => window.__commits);
    const b0 = procStat(bridge.pid);
    const p0 = pageStat();
    counting = true;
    await page.waitForTimeout(idleS * 1000);
    counting = false;
    const b1 = procStat(bridge.pid);
    const p1 = pageStat();
    const perMin = 60 / idleS;
    report[name] = {
      api_calls_per_min: r1([...calls.values()].reduce((s, c) => s + c.n, 0) * perMin),
      api_kb_per_min: r1((bytes / 1024) * perMin),
      react_commits_per_min: r1(((await page.evaluate(() => window.__commits)) - commits0) * perMin),
      bridge_cpu_ms_per_min: Math.round((b1.cpu - b0.cpu) * 1000 * perMin),
      bridge_wakeups_per_min: Math.round((b1.wakeups - b0.wakeups) * perMin),
      page_cpu_ms_per_min: Math.round((p1.cpu - p0.cpu) * 1000 * perMin),
      by_call: Object.fromEntries([...calls].sort((a, b) => b[1].n - a[1].n).map(([k, v]) => [k, { per_min: r1(v.n * perMin), kb_per_min: r1((v.bytes / 1024) * perMin) }])),
    };
  };
  const hide = (hidden) =>
    page.evaluate((hidden) => {
      window.__hidden = hidden;
      document.dispatchEvent(new Event('visibilitychange'));
    }, hidden);
  // --idle 0 skips straight to the scroll.
  if (idleS > 0) {
    await measure('idle');
    await hide(true);
    await measure('hidden');
    await hide(false);
  }

  // Scroll the whole grid, then let it settle.
  images = 0;
  imageBytes = 0;
  const grid = await page.evaluateHandle(() => [...document.querySelectorAll('*')].find((e) => e.scrollHeight > e.clientHeight + 100 && getComputedStyle(e).overflowY !== 'visible' && e.querySelector('.layer img')));
  const steps = await grid.evaluate((g) => Math.ceil(g.scrollHeight / g.clientHeight));
  const cdp = await ctx.newCDPSession(page);
  if (profile) {
    await cdp.send('Profiler.enable');
    await cdp.send('Profiler.setSamplingInterval', { interval: 200 });
    await cdp.send('Profiler.start');
  }
  const s0 = pageStat();
  const ts = Date.now();
  for (let i = 0; i <= steps; i++) {
    await grid.evaluate((g, i) => (g.scrollTop = i * g.clientHeight), i);
    // As a person browsing would: each screen's thumbnails load before the next.
    await page.waitForFunction(() => !document.querySelector('.photo > .shimmer') && [...document.querySelectorAll('.layer img')].every((i) => i.complete), null, { timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(100);
  }
  const scrollMs = Date.now() - ts;
  await page.waitForTimeout(2000);
  const s1 = pageStat();
  if (profile) {
    // Where the page's own time went while scrolling: the top functions by self time.
    const { profile: prof } = await cdp.send('Profiler.stop');
    writeFileSync(join(out, `${label}.cpuprofile`), JSON.stringify(prof));
    const self = new Map();
    const dt = new Map();
    prof.samples.forEach((id, i) => dt.set(id, (dt.get(id) ?? 0) + (prof.timeDeltas[i] ?? 0)));
    for (const n of prof.nodes) {
      const f = n.callFrame;
      const k = `${f.functionName || '(anon)'} ${f.url.split('/').pop()}:${f.lineNumber}`;
      self.set(k, (self.get(k) ?? 0) + (dt.get(n.id) ?? 0) / 1000);
    }
    report.scroll_profile_ms = Object.fromEntries([...self].sort((a, b) => b[1] - a[1]).slice(0, 15).map(([k, v]) => [k, Math.round(v)]));
  }
  await cdp.send('HeapProfiler.collectGarbage');
  const { usedSize } = await cdp.send('Runtime.getHeapUsage');
  report.scroll = {
    screens: steps,
    images_loaded: images,
    image_mb: r1(imageBytes / 1048576),
    page_cpu_ms: Math.round((s1.cpu - s0.cpu) * 1000),
    page_cpu_ms_per_screen: Math.round(((s1.cpu - s0.cpu) * 1000) / steps),
    wall_ms: scrollMs,
    page_rss_mb: Math.round(s1.rss),
    js_heap_mb: r1(usedSize / 1048576),
  };
  report.bridge_rss_mb = Math.round(procStat(bridge.pid).rss);
} finally {
  await browser.close();
  await new Promise((r) => server.httpServer.close(r));
  bridge.kill();
}
writeFileSync(join(out, `${label}.json`), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
