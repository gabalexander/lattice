import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

// In development the API comes from the mock server (`npm run mock`), or from a real lattice when
// LATTICE_URL names one. A real lattice refuses a POST from another origin, so the proxy says it comes from
// the lattice itself.
const api = process.env.LATTICE_URL || 'http://127.0.0.1:7348';

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    proxy: {
      '/api': { target: api, changeOrigin: true, headers: { origin: api } },
      '/assets': { target: api, changeOrigin: true },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
