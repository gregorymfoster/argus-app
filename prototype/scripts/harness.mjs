// What the live checks share: a throwaway Library, the engine behind `argus serve`, the
// dev build and a browser. Each piece used to be copied into every script, unevenly (some
// never killed the engine on a crash, so the next run found its port taken). Adopt it one
// script at a time; layout-lib.mjs is the same idea for the layout checks.

import { createServer } from 'vite';
import { chromium, webkit } from 'playwright';
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, utimesSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const ROOT = new URL('../../', import.meta.url).pathname;
/** The argus binary the checks drive: ARGUS_BIN, else the fast build. */
export const BIN = process.env.ARGUS_BIN ?? join(ROOT, 'target/fast/argus');

/**
 * A new Library of one Shoot, `shoot`, imported from Greg's bench edits: `frames` is a list of
 * [bench file, name in the Shoot], 20 s apart in capture order. Returns its folder.
 */
export function benchLibrary(prefix, shoot, frames) {
  const tmp = mkdtempSync(join(tmpdir(), `argus-${prefix}-`));
  const src = join(tmp, shoot);
  mkdirSync(src);
  const t0 = 1_792_000_000;
  frames.forEach(([from, to], i) => {
    copyFileSync(join(ROOT, 'fixtures/bench/edit/greg', from), join(src, to));
    utimesSync(join(src, to), t0 + i * 20, t0 + i * 20);
  });
  const library = join(tmp, 'lib');
  execFileSync(BIN, ['tool', 'import_folder', JSON.stringify({ folder: src }), '--library', library], { stdio: 'ignore' });
  return library;
}

/**
 * `argus serve` on `library` at `port`, answering once this resolves (it throws if the engine
 * never does). The engine gets a private model lock and Foreground markers, so a cull in
 * Greg's app neither stalls the check nor holds its Queue, and it dies with this process.
 */
export async function startBridge({ library, port, env = {} }) {
  const proc = spawn(BIN, ['serve', '--library', library, '--port', String(port)], {
    env: { ...process.env, ARGUS_MODEL_LOCK_DIR: mkdtempSync(join(tmpdir(), 'argus-model-')), ARGUS_FOREGROUND_DIR: mkdtempSync(join(tmpdir(), 'argus-foreground-')), ...env },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  // A crash must not leave the engine holding its port for the next run ("Address already in use").
  process.on('exit', () => proc.kill());
  const url = `http://127.0.0.1:${port}`;
  for (let i = 0; ; i++) {
    if (await fetch(`${url}/api/list_shoots`).then((r) => r.ok, () => false)) break;
    if (i > 100) throw new Error(`argus serve on ${port} never answered`);
    await new Promise((r) => setTimeout(r, 200));
  }
  return { proc, url };
}

/** The dev build on `port`, sending /api and /previews to the engine on `bridgePort`. */
export async function startVite(port, bridgePort) {
  const engine = `http://127.0.0.1:${bridgePort}`;
  const server = await createServer({ configFile: new URL('../vite.config.ts', import.meta.url).pathname, server: { port, strictPort: true, proxy: { '/api': engine, '/previews': engine } }, logLevel: 'error' });
  await server.listen();
  return server;
}

/** Chromium (PW_CHROMIUM picks the binary), or WebKit, the Mac app's engine, with PW_BROWSER=webkit. */
export function launchBrowser() {
  if (process.env.PW_BROWSER === 'webkit') return webkit.launch();
  return chromium.launch(process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {});
}
