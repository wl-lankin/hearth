import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

export default defineConfig({
  root: 'ui',
  define: { __HEARTH_VERSION__: JSON.stringify(pkg.version) },
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1440, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: { outDir: '../dist', emptyOutDir: true, target: 'chrome120', modulePreload: { polyfill: false } },
  // Under vitest, resolve Svelte's browser build: the server build turns $effect, flushSync and untrack into no-ops.
  resolve: process.env.VITEST ? { conditions: ['browser'] } : undefined,
  test: {
    root: 'ui',
    environment: 'node',
    include: ['src/**/*.spec.ts'],
  },
});
