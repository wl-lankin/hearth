// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test } from 'vitest';
import * as api from './api';
import { createSettingsStore } from './settings.svelte';
import Settings from './views/Settings.svelte';
import TitleBar, { platformOf } from './TitleBar.svelte';
import styles from './styles.css?raw';


test('the Settings page has no theme choice', async () => {
  const target = document.createElement('div');
  document.body.append(target);
  const settings = createSettingsStore(api);
  await settings.load();
  const page = mount(Settings, { target, props: { settings } });
  flushSync();
  expect(target.textContent).not.toContain('Theme');
  expect(target.textContent).not.toContain('Light');
  expect(target.textContent).not.toContain('System');
  unmount(page);
});

test('there is one dark token set', () => {
  expect(styles).not.toContain('data-theme');
  expect(styles).toContain('color-scheme: dark');
});

test('the platform comes from the user agent', () => {
  expect(platformOf('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36')).toBe('windows');
  expect(platformOf('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')).toBe('macos');
  expect(platformOf('Mozilla/5.0 (X11; Linux x86_64)')).toBe('other');
});

test('Windows gets Hearth window buttons in a draggable strip; macOS keeps its own', () => {
  const target = document.createElement('div');
  document.body.append(target);
  const win = mount(TitleBar, { target, props: { platform: 'windows' } });
  flushSync();
  expect(target.querySelector('[data-tauri-drag-region]')).not.toBeNull();
  const labels = [...target.querySelectorAll('button')].map((b) => b.getAttribute('aria-label'));
  expect(labels).toEqual(['Minimize', 'Maximize', 'Close']);
  unmount(win);
  const mac = mount(TitleBar, { target, props: { platform: 'macos' } });
  flushSync();
  expect(target.querySelector('[data-tauri-drag-region]')).not.toBeNull();
  expect(target.querySelectorAll('button').length).toBe(0);
  unmount(mac);
});
