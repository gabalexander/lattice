import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // A single-page app: lattice serves index.html for every path that isn't a file or the API, and the app
    // routes from there. Its pages use absolute paths, so it's served at the root.
    adapter: adapter({ pages: 'build', assets: 'build', fallback: 'index.html', strict: true }),
    paths: { relative: false },
    // The app speaks only to the lattice that serves it.
    csp: {
      mode: 'hash',
      directives: {
        'default-src': ['self'],
        'script-src': ['self'],
        'style-src': ['self', 'unsafe-inline'],
        'img-src': ['self', 'data:'],
        'font-src': ['self'],
        'connect-src': ['self'],
        'object-src': ['none'],
        'base-uri': ['none'],
      },
    },
  },
};

export default config;
