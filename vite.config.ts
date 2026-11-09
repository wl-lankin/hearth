import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

export default defineConfig({
  root: 'ui',
  define: { __STOKE_VERSION__: JSON.stringify(pkg.version) },
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1440, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'chrome120',
    modulePreload: { polyfill: false },
    // Two pages: the app and the tray panel.
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL('./ui/index.html', import.meta.url)),
        panel: fileURLToPath(new URL('./ui/panel.html', import.meta.url)),
      },
    },
  },
  // Under vitest, resolve Svelte's browser build: the server build turns $effect, flushSync and untrack into no-ops.
  resolve: process.env.VITEST ? { conditions: ['browser'] } : undefined,
  test: {
    root: 'ui',
    environment: 'node',
    include: ['src/**/*.spec.ts'],
    // The design tests read the stylesheet's text.
    css: { include: [/styles\.css/] },
  },
});
