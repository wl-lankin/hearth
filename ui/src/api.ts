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

let demoTerminal: TerminalStatus = { enabled: false, others: [String.raw`C:\Users\you\.config\herd\bin`] };
export const terminalGet = (): Promise<TerminalStatus> => (demo ? Promise.resolve({ ...demoTerminal }) : core('terminal.get'));
export const terminalSet = async (on: boolean): Promise<TerminalStatus> => {
  if (demo) {
    demoTerminal = { ...demoTerminal, enabled: on };
    return { ...demoTerminal };
  }
  return core('terminal.set', { on });
};
export const jobsList = (): Promise<JobStatus[]> => (demo ? Promise.resolve([]) : core('jobs.list'));

export interface Site {
  name: string;
  kind: 'parked' | 'linked' | 'proxy';
  path: string | null;
  upstream: string | null;
  docroot: string | null;
  php: string | null;
  secured: boolean;
  problem: string | null;
}

export interface SitesList {
  sites: Site[];
  parked: string[];
  httpPort: number;
  httpsPort: number;
}

export interface WebStatus {
  httpPort: number;
  httpsPort: number;
  /** Browsers trust Hearth's certificates. */
  caTrusted: boolean;
  /** Whether <site>.test resolves; no problem and not ready means this home leaves names alone. */
  dns: { ready: boolean; problem: string | null };
  running: boolean;
  processes: { key: string; running: boolean; pid: number | null; restarts: number; lastError: string | null }[];
  error: string | null;
  notes: Record<string, string>;
}

const demoSites: SitesList = {
  httpPort: 8080,
  httpsPort: 8443,
  parked: [String.raw`C:\Users\you\Projects`],
  sites: [
    { name: 'blog', kind: 'parked', path: String.raw`C:\Users\you\Projects\blog`, upstream: null, docroot: String.raw`C:\Users\you\Projects\blog\public`, php: null, secured: false, problem: null },
    { name: 'shop', kind: 'linked', path: String.raw`C:\Users\you\Code\shop`, upstream: null, docroot: String.raw`C:\Users\you\Code\shop\public`, php: '8.3', secured: true, problem: null },
    { name: 'app.shop', kind: 'proxy', path: null, upstream: 'http://localhost:5173', docroot: null, php: null, secured: false, problem: null },
    { name: 'notes', kind: 'parked', path: String.raw`C:\Users\you\Projects\notes`, upstream: null, docroot: null, php: null, secured: false, problem: 'No index.php or index.html found' },
  ],
};

export const sitesList = (): Promise<SitesList> => (demo ? Promise.resolve(structuredClone(demoSites)) : core('sites.list'));
export const webStatus = (): Promise<WebStatus> =>
  demo
    ? Promise.resolve({ httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: true, processes: [], error: null, notes: {} })
    : core('web.status');
const change = (method: string, params: Record<string, unknown>) => async (): Promise<void> => {
  if (!demo) await core(method, params);
};
export const sitesPark = (dir: string) => change('sites.park', { dir })();
export const sitesUnpark = (dir: string) => change('sites.unpark', { dir })();
export const sitesLink = (name: string, dir: string) => change('sites.link', { name, dir })();
export const sitesUnlink = (name: string) => change('sites.unlink', { name })();
export const sitesProxy = (name: string, upstream: string) => change('sites.proxy', { name, upstream })();
export const sitesUnproxy = (name: string) => change('sites.unproxy', { name })();
export const sitesIsolate = (name: string, minor: string | null) => change('sites.isolate', { name, minor })();
export const sitesPort = (port: number) => change('sites.port', { port })();
export const sitesSecure = (name: string) => change('sites.secure', { name })();
export const sitesUnsecure = (name: string) => change('sites.unsecure', { name })();
export const tlsTrust = () => change('tls.trust', {})();
export const dnsSetup = () => change('dns.setup', {})();

/** A folder picked in the system dialog, or null when cancelled. */
export async function pickFolder(): Promise<string | null> {
  if (demo) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === 'string' ? picked : null;
}

export async function openUrl(url: string): Promise<void> {
  if (demo) return void window.open(url, '_blank');
  await invoke('open_url', { url });
}

export interface ServiceRow {
  id: string;
  kind: string;
  name: string;
  line: string;
  port: number;
  autostart: boolean;
  running: boolean;
  pid: number | null;
  error: string | null;
  log: string;
}

export interface ServicesAvailable {
  builds: { kind: string; line: string; version: string; name: string }[];
  unavailable: { kind: string; name: string; reason: string }[];
}

const demoServices: ServiceRow[] = [
  { id: 'mysql', kind: 'mysql', name: 'MySQL 8.4', line: '8.4', port: 3307, autostart: true, running: true, pid: 4100, error: null, log: '' },
  { id: 'redis', kind: 'redis', name: 'Redis 8', line: '8', port: 6380, autostart: true, running: true, pid: 4101, error: null, log: '' },
  { id: 'postgresql', kind: 'postgresql', name: 'PostgreSQL 18', line: '18', port: 5432, autostart: false, running: false, pid: null, error: null, log: '' },
  { id: 'meilisearch', kind: 'meilisearch', name: 'Meilisearch 1', line: '1', port: 7700, autostart: false, running: false, pid: null, error: 'Port 7700 is used by another program (7701 is free)', log: '' },
];
const demoServicesAvailable: ServicesAvailable = {
  builds: [
    ['mysql', '8.4', '8.4.11', 'MySQL'], ['mysql', '9.7', '9.7.4', 'MySQL'],
    ['mariadb', '11.8', '11.8.9', 'MariaDB'], ['mariadb', '12.3', '12.3.3', 'MariaDB'],
    ['postgresql', '18', '18.6.1', 'PostgreSQL'], ['postgresql', '17', '17.11.1', 'PostgreSQL'],
    ['redis', '8', '8.10.3', 'Redis'], ['meilisearch', '1', '1.56.0', 'Meilisearch'], ['silo', 'current', '20261026000000.0.0', 'Silo (S3)'],
  ].map(([kind, line, version, name]) => ({ kind, line, version, name })),
  unavailable: [{ kind: 'typesense', name: 'Typesense', reason: 'Not available on Windows (it needs WSL)' }],
};

export const servicesList = (): Promise<ServiceRow[]> => (demo ? Promise.resolve(structuredClone(demoServices)) : core('services.list'));
export const servicesAvailable = (): Promise<ServicesAvailable> =>
  demo ? Promise.resolve(structuredClone(demoServicesAvailable)) : core('services.available');
export const servicesSuggestPort = (kind: string): Promise<number> => (demo ? Promise.resolve(3308) : core('services.suggestPort', { kind }));
export const servicesCreate = (kind: string, line: string, port: number, name?: string) =>
  change('services.create', { kind, line, port, name: name || null })();
export const servicesStart = (id: string) => change('services.start', { id })();
export const servicesStop = (id: string) => change('services.stop', { id })();
export const servicesDelete = (id: string) => change('services.delete', { id })();
export const servicesAutostart = (id: string, on: boolean) => change('services.autostart', { id, on })();
export const servicesEnv = (id: string): Promise<string> =>
  demo
    ? Promise.resolve(['DB_CONNECTION=mysql', 'DB_HOST=127.0.0.1', 'DB_PORT=3307', 'DB_USERNAME=root', 'DB_PASSWORD='].join('\n'))
    : core('services.env', { id });
export const servicesLog = (id: string): Promise<string> => (demo ? Promise.resolve('ready for connections') : core('services.log', { id }));
