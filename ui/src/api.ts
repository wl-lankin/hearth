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

export type UpdateState =
  | { phase: 'idle' }
  | { phase: 'disabled' }
  | { phase: 'checking' }
  | { phase: 'current' }
  | { phase: 'downloading'; version: string; progress: number }
  | { phase: 'ready'; version: string }
  | { phase: 'installing'; version: string }
  | { phase: 'error'; message: string };

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

export async function updateState(): Promise<UpdateState> {
  if (demo) return { phase: 'current' };
  return invoke<UpdateState>('update_state');
}

/** Calls `onChange` with every new updater state; returns the unsubscribe function. */
export async function onUpdateState(onChange: (state: UpdateState) => void): Promise<() => void> {
  if (demo) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<UpdateState>('update-state', (event) => onChange(event.payload));
}

export async function checkForUpdates(): Promise<void> {
  if (demo) return;
  await invoke('update_check');
}

export async function installUpdateNow(): Promise<void> {
  if (demo) return;
  await invoke('update_install_now');
}
