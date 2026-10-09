// The exported wiki's build (src/export/main.ts): one classic script, wiki.js, and wiki.css in
// build/export/, with the page's index.html, the fonts and the theme's script beside them, every path
// relative, so the folder works opened from a file and served from anywhere. docs/web.md says how lattice
// fills it in.

import { svelte } from '@sveltejs/vite-plugin-svelte';
import { cpSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));
const out = here('./build/export');

// The page itself, which the build doesn't make: the bundle is a library's, not an app's.
const page = (): Plugin => ({
  name: 'lattice-export-page',
  closeBundle: () => cpSync(here('./src/export/index.html'), `${out}/index.html`),
});

export default defineConfig({
  plugins: [svelte(), page()],
  resolve: { alias: { $lib: here('./src/lib') } },
  // static/ (the fonts, the theme's script, the icon) is copied beside the bundle, and the stylesheet's
  // /fonts/ made relative to it.
  publicDir: here('./static'),
  base: './',
  build: {
    outDir: out,
    emptyOutDir: true,
    target: 'es2022',
    assetsDir: '',
    assetsInlineLimit: 0,
    lib: { entry: here('./src/export/main.ts'), formats: ['iife'], name: 'latticeWiki', fileName: () => 'wiki.js', cssFileName: 'wiki' },
  },
});
