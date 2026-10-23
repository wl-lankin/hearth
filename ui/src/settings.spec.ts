// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { Settings } from './api';
import { createSettingsStore } from './settings.svelte';

const defaults: Settings = { theme: 'system', launchAtLogin: false, startHidden: false };

function fakeApi(fail?: string) {
  const saved: Settings[] = [];
  return {
    saved,
    getSettings: async () => ({ ...defaults }),
    setSettings: async (s: Settings) => {
      if (fail) throw fail;
      saved.push(s);
      return s;
    },
  };
}

describe('settings store', () => {
  test('load fills the value from the api', async () => {
    const api = fakeApi();
    api.getSettings = async () => ({ ...defaults, theme: 'light' });
    const store = createSettingsStore(api);
    await store.load();
    expect(store.value.theme).toBe('light');
  });

  test('update saves the merged settings and keeps them', async () => {
    const api = fakeApi();
    const store = createSettingsStore(api);
    await store.load();
    await store.update({ theme: 'dark' });
    expect(api.saved).toEqual([{ theme: 'dark', launchAtLogin: false, startHidden: false }]);
    expect(store.value.theme).toBe('dark');
    expect(store.error).toBeNull();
  });

  test('a refused update rolls back and shows the reason', async () => {
    const api = fakeApi('Could not change Start at login: denied');
    const store = createSettingsStore(api);
    await store.load();
    await store.update({ launchAtLogin: true });
    expect(store.value.launchAtLogin).toBe(false);
    expect(store.error).toBe('Could not change Start at login: denied');
  });

  test('a later successful update clears the error', async () => {
    const api = fakeApi('nope');
    const store = createSettingsStore(api);
    await store.load();
    await store.update({ startHidden: true });
    api.setSettings = async (s: Settings) => s;
    await store.update({ startHidden: true });
    expect(store.error).toBeNull();
    expect(store.value.startHidden).toBe(true);
  });
});
