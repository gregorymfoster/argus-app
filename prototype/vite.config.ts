import path from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { viteSingleFile } from 'vite-plugin-singlefile';

// `--mode single` inlines everything into dist-single/index.html so the
// prototype can be shared as one file (or published as an Artifact).
export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss(), ...(mode === 'single' ? [viteSingleFile()] : [])],
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  base: './',
  build: { outDir: mode === 'single' ? 'dist-single' : 'dist' },
  // Live mode (?library=live) talks to `argus serve` through these proxies.
  server: {
    port: 5173,
    // The Preset library is shared with the engine (engine/presets/builtin.json).
    fs: { allow: ['.', '../engine/presets'] },
    proxy: { '/api': 'http://127.0.0.1:7878', '/previews': 'http://127.0.0.1:7878' },
  },
}));
