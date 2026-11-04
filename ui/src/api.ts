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
  installed: [{ minor: '8.4', version: '8.4.27', extensions: { redis: true, xdebug: false, spx: true } }],
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

export interface InstalledNode {
  version: string;
  major: number;
  lts: string | null;
}

export interface NodeList {
  installed: InstalledNode[];
  default: string | null;
}

export interface NodeBuild {
  major: number;
  version: string;
  lts: string | null;
  installed: boolean;
}

/** The Node version a folder runs, the version file that asks for it, or why it cannot run. */
export interface NodeWhich {
  version: string | null;
  file: string | null;
  error: string | null;
}

let demoNode: NodeList = { default: '22.21.1', installed: [{ version: '22.21.1', major: 22, lts: 'Jod' }] };
const demoNodeAvailable: NodeBuild[] = [
  { major: 26, version: '26.13.0', lts: 'Lithium', installed: false },
  { major: 25, version: '25.9.0', lts: null, installed: false },
  { major: 24, version: '24.22.2', lts: 'Krypton', installed: false },
  { major: 22, version: '22.23.4', lts: 'Jod', installed: false },
  { major: 20, version: '20.20.2', lts: 'Iron', installed: false },
];

export const nodeList = (): Promise<NodeList> => (demo ? Promise.resolve(structuredClone(demoNode)) : core('node.list'));
export const nodeAvailable = (): Promise<NodeBuild[]> =>
  demo ? Promise.resolve(structuredClone(demoNodeAvailable)) : core('node.available');
export const nodeInstall = async (spec: string): Promise<number> =>
  demo ? 1 : (await core<{ jobId: number }>('node.install', { spec })).jobId;
export const nodeUninstall = async (version: string): Promise<void> => {
  if (!demo) await core('node.uninstall', { spec: version });
};
export const nodeDefault = async (version: string): Promise<void> => {
  if (demo) demoNode = { ...demoNode, default: version };
  else await core('node.default', { spec: version });
};
export const nodeWhich = (dir: string): Promise<NodeWhich> =>
  demo ? Promise.resolve({ version: null, file: null, error: null }) : core('node.which', { dir });

export interface Site {
  name: string;
  kind: 'parked' | 'linked' | 'proxy';
  path: string | null;
  upstream: string | null;
  docroot: string | null;
  php: string | null;
  secured: boolean;
  /** Every request of this site starts an Xdebug session. */
  debug: boolean;
  /** SPX profiles every request of this site. */
  profile: boolean;
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
    { name: 'blog', kind: 'parked', path: String.raw`C:\Users\you\Projects\blog`, upstream: null, docroot: String.raw`C:\Users\you\Projects\blog\public`, php: null, secured: false, debug: false, profile: false, problem: null },
    { name: 'shop', kind: 'linked', path: String.raw`C:\Users\you\Code\shop`, upstream: null, docroot: String.raw`C:\Users\you\Code\shop\public`, php: '8.3', secured: true, debug: true, profile: true, problem: null },
    { name: 'app.shop', kind: 'proxy', path: null, upstream: 'http://localhost:5173', docroot: null, php: null, secured: false, debug: false, profile: false, problem: null },
    { name: 'notes', kind: 'parked', path: String.raw`C:\Users\you\Projects\notes`, upstream: null, docroot: null, php: null, secured: false, debug: false, profile: false, problem: 'No index.php or index.html found' },
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

export interface MailStatus {
  port: number;
  listening: boolean;
  error: string | null;
  /** The .env lines that send an app's mail to Hearth. */
  env: string;
}

export interface MailSummary {
  id: string;
  /** The SMTP username the app sent with (`MAIL_USERNAME`), if any. */
  site: string | null;
  from: string;
  to: string[];
  subject: string;
  /** Unix seconds. */
  received: number;
  size: number;
  attachments: number;
  seen: boolean;
}

export interface MailAttachment {
  index: number;
  name: string;
  contentType: string;
  size: number;
}

export interface Mail extends MailSummary {
  cc: string[];
  html: string | null;
  text: string | null;
  files: MailAttachment[];
}

const demoMailEnv = ['MAIL_MAILER=smtp', 'MAIL_HOST=127.0.0.1', 'MAIL_PORT=2525', 'MAIL_USERNAME="${APP_NAME}"', 'MAIL_PASSWORD=null'].join('\n');
const demoNow = Math.floor(Date.now() / 1000);
let demoMails: Mail[] = [
  {
    id: '1800000000003-0000', site: 'shop', from: 'Shop <hello@shop.test>', to: ['ann@example.test'], subject: 'Your order #1042 is on its way',
    received: demoNow - 120, size: 48210, attachments: 1, seen: false, cc: [],
    html: '<div style="font-family:sans-serif;max-width:520px;margin:24px auto;color:#2a2420"><h2 style="color:#e8642c">Thanks for your order, Ann</h2><p>Order #1042 left our warehouse today.</p><p><a href="https://shop.test/orders/1042" style="display:inline-block;padding:10px 18px;background:#e8642c;color:#fff;border-radius:8px;text-decoration:none">Track your parcel</a></p><p style="color:#7a6d64;font-size:13px">Questions? Visit <a href="https://shop.test/help">our help pages</a>.</p></div>',
    text: 'Thanks for your order, Ann\n\nOrder #1042 left our warehouse today.\nTrack it: https://shop.test/orders/1042',
    files: [{ index: 0, name: 'invoice-1042.pdf', contentType: 'application/pdf', size: 41230 }],
  },
  {
    id: '1800000000002-0000', site: 'shop', from: 'Shop <hello@shop.test>', to: ['ann@example.test'], subject: 'Reset your password',
    received: demoNow - 3600, size: 2210, attachments: 0, seen: true, cc: [],
    html: '<p>Someone asked to reset your password.</p><p><a href="https://shop.test/reset/9f2c">Choose a new password</a></p>',
    text: 'Someone asked to reset your password.\nhttps://shop.test/reset/9f2c', files: [],
  },
  {
    id: '1800000000001-0000', site: null, from: 'cron@localhost', to: ['dev@example.test'], subject: 'Nightly import finished',
    received: demoNow - 86400, size: 640, attachments: 0, seen: false, cc: [], html: null,
    text: 'Imported 1,284 rows in 3.2 s.\nNo errors.', files: [],
  },
];
const demoSummary = (m: Mail): MailSummary => {
  const { cc: _cc, html: _html, text: _text, files: _files, ...summary } = m;
  return summary;
};

export const mailStatus = (): Promise<MailStatus> =>
  demo ? Promise.resolve({ port: 2525, listening: true, error: null, env: demoMailEnv }) : core('mail.status');
export const mailList = (): Promise<MailSummary[]> => (demo ? Promise.resolve(demoMails.map(demoSummary)) : core('mail.list'));
export const mailGet = (id: string): Promise<Mail> => {
  if (!demo) return core('mail.get', { id });
  demoMails = demoMails.map((m) => (m.id === id ? { ...m, seen: true } : m));
  return Promise.resolve(structuredClone(demoMails.find((m) => m.id === id)!));
};
export const mailRaw = (id: string): Promise<{ text: string; truncated: boolean }> =>
  demo ? Promise.resolve({ text: `Subject: ${demoMails.find((m) => m.id === id)?.subject}\r\n\r\n...`, truncated: false }) : core('mail.raw', { id });
export const mailDelete = async (id: string): Promise<void> => {
  if (demo) demoMails = demoMails.filter((m) => m.id !== id);
  else await core('mail.delete', { id });
};
export const mailClear = async (site: string | null | undefined): Promise<void> => {
  if (demo) demoMails = site === undefined ? [] : demoMails.filter((m) => m.site !== site);
  else await core('mail.clear', site === undefined ? {} : { site });
};
export const mailPort = (port: number): Promise<MailStatus> =>
  demo ? Promise.resolve({ port, listening: true, error: null, env: demoMailEnv.replace('2525', String(port)) }) : core('mail.port', { port });
export const mailSaveAttachment = (id: string, index: number, path: string) => change('mail.saveAttachment', { id, index, path })();

/** Where to save a file, picked in the system dialog; null when cancelled. */
export async function pickSavePath(name: string): Promise<string | null> {
  if (demo) return null;
  const { save } = await import('@tauri-apps/plugin-dialog');
  return save({ defaultPath: name });
}

export interface DumpsStatus {
  port: number;
  listening: boolean;
  error: string | null;
  /** Whether dump() and dd() are sent to Hearth. */
  enabled: boolean;
  /** Whether Laravel requests and their queries are recorded. */
  requests: boolean;
}

export interface Dump {
  id: number;
  /** Unix milliseconds. */
  time: number;
  site: string | null;
  file: string | null;
  line: number | null;
  sapi: string;
  uri: string | null;
  text: string;
  truncated: boolean;
}

const demoMs = Date.now();
let demoDumps: Dump[] = [
  {
    id: 3, time: demoMs - 4000, site: 'shop.test', file: String.raw`C:\Users\you\Projects\shop\app\Http\Controllers\CartController.php`, line: 42,
    sapi: 'cgi-fcgi', uri: '/cart?coupon=AUTUMN', truncated: false,
    text: String.raw`App\Models\Cart {#1842
  #attributes: array:4 [
    "id" => 17
    "user_id" => 3
    "total" => "84.90"
    "coupon" => "AUTUMN"
  ]
  #relations: array:1 [
    "items" => Illuminate\Database\Eloquent\Collection {#1850 …2}
  ]
}`,
  },
  {
    id: 2, time: demoMs - 65000, site: 'shop.test', file: String.raw`C:\Users\you\Projects\shop\app\Services\Pricing.php`, line: 18,
    sapi: 'cgi-fcgi', uri: '/cart?coupon=AUTUMN', truncated: false, text: '84.9',
  },
  {
    id: 1, time: demoMs - 3600000, site: 'notes', file: String.raw`C:\Users\you\Projects\notes\routes\console.php`, line: 9,
    sapi: 'cli', uri: null, truncated: false, text: 'array:2 [\n  0 => "inbox"\n  1 => "archive"\n]',
  },
];
let demoDumpsStatus: DumpsStatus = { port: 2306, listening: true, error: null, enabled: true, requests: true };

export const dumpsStatus = (): Promise<DumpsStatus> => (demo ? Promise.resolve({ ...demoDumpsStatus }) : core('dumps.status'));
export const dumpsList = (after?: number): Promise<Dump[]> =>
  demo
    ? Promise.resolve(structuredClone(demoDumps.filter((d) => after === undefined || d.id > after)))
    : core('dumps.list', after === undefined ? {} : { after });
export const dumpsClear = async (): Promise<void> => {
  if (demo) demoDumps = [];
  else await core('dumps.clear');
};
export const dumpsEnable = (on: boolean): Promise<DumpsStatus> => {
  if (!demo) return core('dumps.enable', { on });
  demoDumpsStatus = { ...demoDumpsStatus, enabled: on };
  return Promise.resolve({ ...demoDumpsStatus });
};
export const dumpsPort = (port: number): Promise<DumpsStatus> => {
  if (!demo) return core('dumps.port', { port });
  demoDumpsStatus = { ...demoDumpsStatus, port };
  return Promise.resolve({ ...demoDumpsStatus });
};

export interface RequestSummary {
  id: number;
  /** Unix milliseconds. */
  time: number;
  site: string | null;
  sapi: string;
  method: string | null;
  uri: string | null;
  /** The artisan command, for command-line runs. */
  command: string | null;
  status: number | null;
  ms: number;
  memory: number;
  queryCount: number;
  queryMs: number;
  /** Queries over the 500 kept. */
  dropped: number;
  /** The kinds found: n+1, duplicate, slow. */
  findings: string[];
}

export interface RequestQuery {
  sql: string;
  bindings: string[];
  ms: number;
  /** ms after the request started. */
  startMs: number;
  connection: string;
  file: string | null;
  line: number | null;
}

export interface RequestFinding {
  kind: 'n+1' | 'duplicate' | 'slow';
  text: string;
  /** Indexes into the queries. */
  queries: number[];
}

export interface RequestDetail extends Omit<RequestSummary, 'findings' | 'queryCount' | 'queryMs'> {
  queries: RequestQuery[];
  findings: RequestFinding[];
}

const demoCart = String.raw`C:\Users\you\Projects\shop\app\Http\Controllers\CartController.php`;
const demoItems = 'select * from "items" where "items"."cart_id" = ? and "items"."cart_id" is not null';
const demoUser = 'select * from "users" where "id" = ? limit 1';
const demoRequestDetail: RequestDetail = {
  id: 7, time: Date.now() - 20000, site: 'shop.test', sapi: 'cgi-fcgi', method: 'GET', uri: '/cart', command: null, status: 200,
  ms: 186.4, memory: 18874368, dropped: 0,
  queries: [
    { sql: demoUser, bindings: ['3'], ms: 0.9, startMs: 12.1, connection: 'sqlite', file: demoCart, line: 18 },
    { sql: 'select * from "carts" where "user_id" = ?', bindings: ['3'], ms: 1.2, startMs: 14.0, connection: 'sqlite', file: demoCart, line: 21 },
    ...[17, 18, 19, 20, 21, 22].map((id, i) => ({
      sql: demoItems, bindings: [String(id)], ms: 1.1, startMs: 18 + i * 4, connection: 'sqlite', file: demoCart, line: 26,
    })),
    { sql: demoUser, bindings: ['3'], ms: 0.8, startMs: 44.0, connection: 'sqlite', file: demoCart, line: 31 },
    { sql: 'select sum("total") as aggregate from "orders" where "user_id" = ?', bindings: ['3'], ms: 128.5, startMs: 48.0, connection: 'sqlite', file: demoCart, line: 34 },
  ],
  findings: [
    { kind: 'n+1', text: `Ran 6 times from CartController.php:26: ${demoItems}`, queries: [2, 3, 4, 5, 6, 7] },
    { kind: 'duplicate', text: `The same query ran 2 times, first from CartController.php:18: ${demoUser}`, queries: [0, 8] },
    { kind: 'slow', text: 'Took 129 ms at CartController.php:34: select sum("total") as aggregate from "orders" where "user_id" = ?', queries: [9] },
  ],
};
const demoSummaryOf = (r: RequestDetail): RequestSummary => ({
  ...r,
  queryCount: r.queries.length,
  queryMs: r.queries.reduce((n, q) => n + q.ms, 0),
  findings: [...new Set(r.findings.map((f) => f.kind))],
});
let demoRequests: RequestDetail[] = [
  demoRequestDetail,
  { ...demoRequestDetail, id: 6, time: Date.now() - 90000, method: 'POST', uri: '/cart/items', status: 302, ms: 38.2, queries: demoRequestDetail.queries.slice(0, 2), findings: [] },
  {
    ...demoRequestDetail, id: 5, time: Date.now() - 400000, site: 'shop', sapi: 'cli', method: null, uri: null, command: 'migrate --seed',
    status: null, ms: 912.7, queries: demoRequestDetail.queries.slice(0, 2), findings: [],
  },
];

export const requestsList = (after?: number): Promise<RequestSummary[]> =>
  demo
    ? Promise.resolve(demoRequests.filter((r) => after === undefined || r.id > after).map(demoSummaryOf))
    : core('requests.list', after === undefined ? {} : { after });
export const requestsGet = (id: number): Promise<RequestDetail> =>
  demo ? Promise.resolve(structuredClone(demoRequests.find((r) => r.id === id)!)) : core('requests.get', { id });
export const requestsClear = async (): Promise<void> => {
  if (demo) demoRequests = [];
  else await core('requests.clear');
};
export const requestsEnable = (on: boolean): Promise<DumpsStatus> => {
  if (!demo) return core('requests.enable', { on });
  demoDumpsStatus = { ...demoDumpsStatus, requests: on };
  return Promise.resolve({ ...demoDumpsStatus });
};

export interface LogFile {
  name: string;
  size: number;
  /** Unix milliseconds. */
  modified: number;
}

export interface SiteLogs {
  site: string;
  /** Newest first. */
  files: LogFile[];
}

export interface LogEntry {
  time: string;
  env: string;
  /** Lowercase: debug to emergency. */
  level: string;
  /** The first line. */
  message: string;
  /** Context and stack trace. */
  detail: string;
  truncated: boolean;
}

export interface LogRead {
  size: number;
  entries?: LogEntry[];
  unchanged?: boolean;
}

const demoLogSites: SiteLogs[] = [
  { site: 'shop', files: [{ name: 'laravel.log', size: 48213, modified: Date.now() - 60000 }] },
  { site: 'notes', files: [{ name: 'laravel-2026-11-03.log', size: 1200, modified: Date.now() - 3600000 }] },
];
let demoLogEntries: LogEntry[] = [
  {
    time: '2026-11-03 15:42:09', env: 'local', level: 'error', truncated: false,
    message: 'Division by zero {"userId":3,"exception":"[object] (DivisionByZeroError(code: 0): Division by zero at C:\\Users\\you\\Projects\\shop\\app\\Services\\Pricing.php:18)',
    detail: String.raw`[stacktrace]
#0 C:\Users\you\Projects\shop\app\Http\Controllers\CartController.php(40): App\Services\Pricing->total(Object(App\Models\Cart))
#1 C:\Users\you\Projects\shop\vendor\laravel\framework\src\Illuminate\Routing\Controller.php(54): App\Http\Controllers\CartController->show()
#2 C:\Users\you\Projects\shop\public\index.php(20): Illuminate\Foundation\Application->handleRequest(Object(Illuminate\Http\Request))
#3 {main}
"}`,
  },
  { time: '2026-11-03 15:41:55', env: 'local', level: 'warning', truncated: false, message: 'Slow cart total {"userId":3,"ms":812}', detail: '' },
  { time: '2026-11-03 15:40:12', env: 'local', level: 'info', truncated: false, message: 'User 3 logged in', detail: '' },
  { time: '2026-11-03 15:40:11', env: 'local', level: 'debug', truncated: false, message: 'Session started', detail: '' },
];

export const logsSites = (): Promise<SiteLogs[]> => (demo ? Promise.resolve(structuredClone(demoLogSites)) : core('logs.sites'));
export const logsRead = (site: string, file: string, size?: number): Promise<LogRead> =>
  demo
    ? Promise.resolve(size === 48213 ? { size, unchanged: true } : { size: 48213, entries: structuredClone(demoLogEntries) })
    : core('logs.read', size === undefined ? { site, file } : { site, file, size });
export const logsClear = async (site: string, file: string): Promise<void> => {
  if (demo) demoLogEntries = [];
  else await core('logs.clear', { site, file });
};

export interface DebugStatus {
  port: number;
  listening: boolean;
  /** The listening program's name, when it can be read. */
  program: string | null;
}

export const sitesDebug = (name: string, on: boolean) => change('sites.debug', { name, on })();
export const debugStatus = (): Promise<DebugStatus> =>
  demo ? Promise.resolve({ port: 9003, listening: true, program: 'phpstorm64' }) : core('debug.status');

export const sitesProfile = (name: string, on: boolean) => change('sites.profile', { name, on })();

/** One profiled request or command (SPX report metadata). */
export interface ProfileReport {
  /** SPX's report key; its web UI opens the report by it. */
  key: string;
  /** Unix seconds. */
  time: number;
  /** The site's host without the port; empty for a command. */
  host: string;
  method: string;
  /** The request URI, or the command line of a command-line profile. */
  uri: string;
  cli: boolean;
  wallMs: number;
  /** Bytes. */
  peakMemory: number;
  functions: number;
}

export interface ProfileStatus {
  /** Whether any installed PHP has SPX (Hearth's macOS builds). */
  available: boolean;
  /** The key SPX's web UI asks for. */
  key: string;
}

let demoReports: ProfileReport[] = [
  { key: 'spx-full-20261104_102512-mac-41-1', time: 1793787912, host: 'shop.test', method: 'GET', uri: '/orders?page=2', cli: false, wallMs: 412, peakMemory: 18_874_368, functions: 2841 },
  { key: 'spx-full-20261104_102455-mac-41-2', time: 1793787895, host: 'shop.test', method: 'POST', uri: '/cart/items', cli: false, wallMs: 96, peakMemory: 12_582_912, functions: 1907 },
  { key: 'spx-full-20261104_101803-mac-40-3', time: 1793787483, host: '', method: '', uri: 'php artisan migrate', cli: true, wallMs: 2310, peakMemory: 33_554_432, functions: 4120 },
];

export const profileStatus = (): Promise<ProfileStatus> =>
  demo ? Promise.resolve({ available: true, key: 'demo' }) : core('profile.status');
export const profileList = (): Promise<{ reports: ProfileReport[] }> =>
  demo ? Promise.resolve({ reports: structuredClone(demoReports) }) : core('profile.list');
export const profileClear = async (): Promise<{ deleted: number }> => {
  if (!demo) return core('profile.clear');
  const deleted = demoReports.length;
  demoReports = [];
  return { deleted };
};

export interface ProcessRow {
  name: string;
  command: string;
  autostart: boolean;
  running: boolean;
  pid: number | null;
  restarts: number;
  lastError: string | null;
  /** Crashed: started again after a back-off delay. */
  waiting: boolean;
}

export interface ProcessList {
  sites: { site: string; processes: ProcessRow[] }[];
}

export interface ProcessOutput {
  /** Where the next read starts. */
  offset: number;
  text: string;
  /** The log was emptied (a restart): the text replaces what is shown. */
  reset: boolean;
  /** Which start the log belongs to: a new one means the shown text is from an old run. */
  run: number;
}

export interface ProcessSuggestion {
  name: string;
  command: string;
  autostart: boolean;
}

let demoProcesses: ProcessList = {
  sites: [
    {
      site: 'shop',
      processes: [
        { name: 'vite', command: 'npm run dev', autostart: true, running: true, pid: 4120, restarts: 0, lastError: null, waiting: false },
        { name: 'queue', command: 'php artisan queue:listen --tries=1', autostart: false, running: false, pid: null, restarts: 0, lastError: null, waiting: false },
      ],
    },
  ],
};
const demoOutput = `
> dev
> vite


  VITE v7.1.4  ready in 412 ms

  ➜  Local:   http://localhost:5173/
  ➜  Network: use --host to expose
  ➜  press h + enter to show help

  LARAVEL v12.30.1  plugin v2.0.1

  ➜  APP_URL: http://shop.test:8080
11:02:16 [vite] (client) hmr update /resources/js/app.js
`;

const demoAct = (site: string, name: string, running: boolean) => {
  const row = demoProcesses.sites.find((s) => s.site === site)?.processes.find((p) => p.name === name);
  if (row) row.running = running;
};

export const processesList = (): Promise<ProcessList> =>
  demo ? Promise.resolve(structuredClone(demoProcesses)) : core('processes.list');
export const processesOutput = (site: string, name: string, offset?: number): Promise<ProcessOutput> =>
  demo
    ? Promise.resolve(offset === undefined && site === 'shop' && name === 'vite' ? { offset: demoOutput.length, text: demoOutput, reset: false, run: 1 } : { offset: offset ?? 0, text: '', reset: false, run: 1 })
    : core('processes.output', offset === undefined ? { site, name } : { site, name, offset });
export const processesStart = async (site: string, name: string): Promise<void> => {
  if (demo) return demoAct(site, name, true);
  await core('processes.start', { site, name });
};
export const processesRestart = async (site: string, name: string): Promise<void> => {
  if (demo) return demoAct(site, name, true);
  await core('processes.restart', { site, name });
};
export const processesStop = async (site: string, name: string): Promise<void> => {
  if (demo) return demoAct(site, name, false);
  await core('processes.stop', { site, name });
};
export const processesAdd = async (site: string, name: string, command: string, autostart: boolean): Promise<void> => {
  if (!demo) return void (await core('processes.add', { site, name, command, autostart }));
  let group = demoProcesses.sites.find((s) => s.site === site);
  if (!group) demoProcesses.sites.push((group = { site, processes: [] }));
  group.processes.push({ name, command, autostart, running: false, pid: null, restarts: 0, lastError: null, waiting: false });
};
export const processesRemove = async (site: string, name: string): Promise<void> => {
  if (!demo) return void (await core('processes.remove', { site, name }));
  demoProcesses = { sites: demoProcesses.sites.map((s) => (s.site === site ? { ...s, processes: s.processes.filter((p) => p.name !== name) } : s)).filter((s) => s.processes.length) };
};
export const processesSuggest = (site: string): Promise<ProcessSuggestion[]> =>
  demo
    ? Promise.resolve([
        { name: 'vite', command: 'npm run dev', autostart: false },
        { name: 'queue', command: 'php artisan queue:listen --tries=1', autostart: false },
        { name: 'pail', command: 'php artisan pail', autostart: false },
      ])
    : core('processes.suggest', { site });
export interface Workspace {
  kind: 'worktree';
  /** The branch as typed. */
  name: string;
  /** The host without `.test` (`feature-x.shop`). */
  site: string;
  url: string;
  path: string;
  appDir: string;
  parentSite: string;
  parentPath: string;
  branch: string;
  baseBranch: string;
  baseCommit: string;
  secured: boolean;
  phpVersion: string | null;
  databaseMode: 'shared' | 'empty' | 'clone';
  databaseName: string | null;
  databaseDriver: string | null;
  createdBy: string;
  createdAt: number;
  owned: boolean;
}

const demoWorkspace = (name: string, database: Workspace['databaseMode'], databaseName: string | null): Workspace => ({
  kind: 'worktree', name, site: `${name}.shop`, url: `https://${name}.shop.test:8443`,
  path: `C:\\Users\\you\\.config\\hearth\\data\\workspaces\\shop\\${name}`, appDir: '', parentSite: 'shop',
  parentPath: String.raw`C:\Users\you\Code\shop`, branch: name, baseBranch: 'main', baseCommit: '4f2a9c1', secured: true,
  phpVersion: '8.3', databaseMode: database, databaseName, databaseDriver: databaseName ? 'mysql' : null, createdBy: 'app',
  createdAt: 1793787000, owned: true,
});
let demoWorkspaces: Workspace[] = [demoWorkspace('checkout-v2', 'clone', 'shop_checkout_v2'), demoWorkspace('fix-invoices', 'shared', null)];

export const workspacesList = (): Promise<{ workspaces: Workspace[] }> =>
  demo ? Promise.resolve({ workspaces: structuredClone(demoWorkspaces) }) : core('workspaces.list');
export const workspacesCreate = async (site: string, name: string, database: string, secure?: boolean): Promise<{ workspace: Workspace; notes: string[] }> => {
  if (!demo) return core('workspaces.create', { site, name, database, ...(secure === undefined ? {} : { secure }) });
  const made = demoWorkspace(name || 'brave-otter', database as Workspace['databaseMode'], database === 'shared' ? null : `shop_${name}`);
  demoWorkspaces.push(made);
  return { workspace: made, notes: [] };
};
export const workspacesDelete = async (name: string, keepDatabase: boolean, keepFiles: boolean): Promise<{ notes: string[] }> => {
  if (!demo) return core('workspaces.delete', { name, keepDatabase, keepFiles });
  demoWorkspaces = demoWorkspaces.filter((w) => w.site !== name);
  return { notes: [] };
};
export const workspacesMerge = async (name: string, push: boolean, del: boolean): Promise<{ message: string; notes: string[] }> =>
  demo ? { message: `Merged ${name.split('.')[0]} into main of shop`, notes: [] } : core('workspaces.merge', { name, push, delete: del });
// The tray panel's window and the app around it (nothing to do in the browser demo).
export async function panelFit(width: number, height: number): Promise<void> {
  if (!demo) await invoke('panel_fit', { width, height });
}
export async function panelHide(): Promise<void> {
  if (!demo) await invoke('panel_hide');
}
/** Shows the main window, on `view` (and `tab`) when given. */
export async function appOpen(view?: string, tab?: string): Promise<void> {
  if (!demo) await invoke('app_open', { view: view ?? null, tab: tab ?? null });
}
export async function appQuit(): Promise<void> {
  if (!demo) await invoke('app_quit');
}
