// The wiki's page on its own, for `lattice export`: built by vite.export.config.ts into build/export/ as one
// classic script (no modules, which a page opened from a file can't load), its stylesheet and the fonts.
// lattice copies those and mermaid.min.js beside an index.html with the wiki inlined in its
// `<script type="application/json" id="wiki-data">`, and the page works from file:// and on any static host.

import '../app.css';
import { mount } from 'svelte';
import { setMermaidUrl } from '$lib/mermaid';
import { theme } from '$lib/theme.svelte';
import type { Wiki } from '$lib/types';
import ExportPage from './ExportPage.svelte';

const data = document.getElementById('wiki-data');
const target = document.getElementById('app')!;
const script = document.currentScript as HTMLScriptElement | null;
setMermaidUrl(script ? new URL('mermaid.min.js', script.src).href : 'mermaid.min.js');
theme.apply();

let wiki: Wiki | null = null;
try {
  wiki = JSON.parse(data?.textContent || 'null');
} catch {
  wiki = null;
}
if (wiki && Array.isArray(wiki.sections)) {
  mount(ExportPage, { target, props: { wiki } });
} else {
  target.textContent = 'This page has no wiki in it: lattice export writes one into it.';
}
