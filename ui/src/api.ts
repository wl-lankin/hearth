import { invoke } from '@tauri-apps/api/core';

export type ThemeSetting = 'system' | 'light' | 'dark';

export interface Settings {
  theme: ThemeSetting;
  launchAtLogin: boolean;
  startHidden: boolean;
}

export interface CoreStatus {
  version: string;
  pid: number;
  uptimeSecs: number;
  home: string;
}

/** Outside Tauri (`npm run dev` in a browser) the UI runs on canned data, for layout work and screenshots. */
const demo = typeof window !== 'undefined' && !('__TAURI_INTERNALS__' in window);
const demoStarted = Date.now();
let demoSettings: Settings = { theme: 'system', launchAtLogin: false, startHidden: false };

export async function coreStatus(): Promise<CoreStatus> {
  if (demo) {
    const uptimeSecs = Math.floor((Date.now() - demoStarted) / 1000);
    return { version: __HEARTH_VERSION__, pid: 4242, uptimeSecs, home: '~/.config/hearth' };
  }
  return invoke<CoreStatus>('core_status');
}

export async function getSettings(): Promise<Settings> {
  if (demo) return { ...demoSettings };
  return invoke<Settings>('settings_get');
}

export async function setSettings(settings: Settings): Promise<Settings> {
  if (demo) {
    demoSettings = { ...settings };
    return { ...demoSettings };
  }
  return invoke<Settings>('settings_set', { settings });
}
