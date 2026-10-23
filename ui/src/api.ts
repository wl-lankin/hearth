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

export interface InstalledPhp {
  minor: string;
  version: string;
  extensions: Record<string, boolean>;
}

export interface PhpList {
  default: string | null;
  installed: InstalledPhp[];
}

export interface AvailableBuild {
  minor: string;
  version: string;
  channel: 'stable' | 'rc';
  installedVersion: string | null;
  updateAvailable: boolean;
}

export interface JobStatus {
  id: number;
  kind: string;
  key: string;
  label: string;
  progress: number;
  message: string;
  state: 'running' | 'done' | 'failed';
  error: string | null;
}

/** php.*, jobs.* and tools.* methods of the core, the same ones the CLI uses. */
async function core<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>('core_call', { method, params });
}

const demoPhp: PhpList = {
  default: '8.4',
  installed: [{ minor: '8.4', version: '8.4.27', extensions: { redis: true, xdebug: false } }],
};
const demoAvailable: AvailableBuild[] = [
  { minor: '8.6', version: '8.6.0RC4', channel: 'rc', installedVersion: null, updateAvailable: false },
  { minor: '8.5', version: '8.5.12', channel: 'stable', installedVersion: null, updateAvailable: false },
  { minor: '8.4', version: '8.4.28', channel: 'stable', installedVersion: '8.4.27', updateAvailable: true },
  { minor: '8.3', version: '8.3.36', channel: 'stable', installedVersion: null, updateAvailable: false },
  { minor: '8.2', version: '8.2.35', channel: 'stable', installedVersion: null, updateAvailable: false },
  { minor: '8.1', version: '8.1.34', channel: 'stable', installedVersion: null, updateAvailable: false },
];

export const phpList = (): Promise<PhpList> => (demo ? Promise.resolve(structuredClone(demoPhp)) : core('php.list'));
export const phpAvailable = (): Promise<AvailableBuild[]> =>
  demo ? Promise.resolve(structuredClone(demoAvailable)) : core('php.available');
export const phpInstall = async (minor: string): Promise<number> =>
  demo ? 1 : (await core<{ jobId: number }>('php.install', { minor })).jobId;
export const jobGet = (id: number): Promise<JobStatus> =>
  demo
    ? Promise.resolve({ id, kind: 'php.install', key: '', label: '', progress: 0.45, message: 'Downloading PHP 8.3.36', state: 'running', error: null })
    : core('jobs.get', { id });
export const phpUninstall = async (minor: string): Promise<string | null> =>
  demo ? null : (await core<{ default: string | null }>('php.uninstall', { minor })).default;
export const phpUse = async (minor: string): Promise<void> => {
  if (!demo) await core('php.use', { minor });
};
export const phpExtension = async (minor: string, name: string, on: boolean): Promise<void> => {
  if (!demo) await core('php.extension', { minor, name, on });
};
export const phpIniGet = (minor: string): Promise<{ generated: string; user: string }> =>
  demo
    ? Promise.resolve({ generated: '; Generated by Hearth.\nmemory_limit = 512M\n', user: '; Your own settings for PHP 8.4.\n' })
    : core('php.ini.get', { minor });
export const phpIniSet = async (minor: string, user: string): Promise<void> => {
  if (!demo) await core('php.ini.set', { minor, user });
};
export const phpInstallLaravel = (): Promise<{ jobId: number }> =>
  demo ? Promise.resolve({ jobId: 2 }) : core('tools.laravel');

export interface TerminalStatus {
  enabled: boolean;
  /** Other PATH folders with a php command (e.g. Herd's). */
  others: string[];
}

let demoTerminal: TerminalStatus = { enabled: false, others: ['C:\Users\you\.config\herd\bin'] };
export const terminalGet = (): Promise<TerminalStatus> => (demo ? Promise.resolve({ ...demoTerminal }) : core('terminal.get'));
export const terminalSet = async (on: boolean): Promise<TerminalStatus> => {
  if (demo) {
    demoTerminal = { ...demoTerminal, enabled: on };
    return { ...demoTerminal };
  }
  return core('terminal.set', { on });
};
