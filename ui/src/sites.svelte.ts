import type { DebugStatus, NodeWhich, PhpList, Site, SitesList, WebStatus } from './api';
import { publicPorts, siteUrl } from './tray';

export type SitesApi = {
  sitesList(): Promise<SitesList>;
  webStatus(): Promise<WebStatus>;
  phpList(): Promise<PhpList>;
  sitesPark(dir: string): Promise<void>;
  sitesUnpark(dir: string): Promise<void>;
  sitesLink(name: string, dir: string): Promise<void>;
  sitesUnlink(name: string): Promise<void>;
  sitesProxy(name: string, upstream: string): Promise<void>;
  sitesUnproxy(name: string): Promise<void>;
  sitesIsolate(name: string, minor: string | null): Promise<void>;
  sitesPort(port: number): Promise<void>;
  sitesSecure(name: string): Promise<void>;
  sitesUnsecure(name: string): Promise<void>;
  tlsTrust(): Promise<void>;
  dnsSetup(): Promise<void>;
  /** macOS: ports 80 and 443 forwarded to Stoke, or not. */
  webForward?(on: boolean): Promise<void>;
  sitesDebug(name: string, on: boolean): Promise<void>;
  sitesProfile(name: string, on: boolean): Promise<void>;
  debugStatus(): Promise<DebugStatus>;
  /** Optional: the Node a site's folder runs (shown for sites with a version file). */
  nodeWhich?(dir: string): Promise<NodeWhich>;
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createSitesStore(api: SitesApi) {
  let list = $state<SitesList>({ sites: [], parked: [], httpPort: 8080, httpsPort: 8443 });
  let status = $state<WebStatus | null>(null);
  let php = $state<PhpList>({ default: null, installed: [] });
  let errors = $state<Record<string, string | null>>({});
  let error = $state<string | null>(null);
  let nodes = $state<Record<string, NodeWhich>>({});
  let ide = $state<DebugStatus | null>(null);
  // One IDE check at a time: on macOS it runs lsof, which can be slow.
  let ideBusy = false;

  /** Starts an IDE check unless one runs; resolves when the one it started is done. */
  function checkIde(): Promise<void> {
    if (ideBusy) return Promise.resolve();
    ideBusy = true;
    return api
      .debugStatus()
      .then((s) => {
        ide = s;
      })
      .catch(() => {})
      .finally(() => {
        ideBusy = false;
      });
  }

  async function reload() {
    [list, status, php] = await Promise.all([api.sitesList(), api.webStatus(), api.phpList()]);
    await checkIde();
    const found = await Promise.all(
      list.sites.map(async (site) => [site.name, site.path ? await api.nodeWhich?.(site.path).catch(() => undefined) : undefined] as const),
    );
    nodes = Object.fromEntries(found.filter((f): f is readonly [string, NodeWhich] => !!f[1]));
  }

  /** Flips a site switch at once, then asks the core; a refusal puts it back with the reason. */
  async function toggle(name: string, field: 'debug' | 'profile', on: boolean, f: () => Promise<void>) {
    const site = list.sites.find((s) => s.name === name);
    if (!site) return;
    const before = site[field];
    site[field] = on;
    errors[name] = null;
    try {
      await f();
    } catch (e) {
      site[field] = before;
      errors[name] = asMessage(e);
    }
  }

  /** Runs a change, then reloads; a failure shows on `key` (a site) or as the page error. */
  async function act(key: string | null, f: () => Promise<void>) {
    if (key) errors[key] = null;
    else error = null;
    try {
      await f();
      await reload();
    } catch (e) {
      if (key) errors[key] = asMessage(e);
      else error = asMessage(e);
    }
  }

  return {
    get sites() {
      return list.sites;
    },
    get parked() {
      return list.parked;
    },
    get httpPort() {
      return list.httpPort;
    },
    get status() {
      return status;
    },
    get errors() {
      return errors;
    },
    get error() {
      return error;
    },
    get phpChoices() {
      return php.installed.map((i) => i.minor);
    },
    get defaultPhp() {
      return php.default;
    },
    /** The Node a site runs when its folder has a `.nvmrc` / `.node-version`; null without one. */
    node(name: string): { text: string; problem: string | null } | null {
      const which = nodes[name];
      if (!which?.file) return null;
      return which.version ? { text: `Node ${which.version}`, problem: null } : { text: 'Node not installed', problem: which.error };
    },
    /** The one-line web server state above the sites. */
    get server(): { kind: 'error' | 'running' | 'stopped' | 'idle'; text: string } {
      if (status?.error) return { kind: 'error', text: status.error };
      if (status?.running) {
        return { kind: 'running', text: list.forwarded ? 'Serving on ports 80 and 443' : `Serving on port ${list.httpPort}` };
      }
      if (list.sites.length > 0) return { kind: 'stopped', text: 'Not running yet' };
      return { kind: 'idle', text: 'Not serving yet: park or link a folder' };
    },
    /** The next port when the current one is taken by another program. */
    get portSuggestion(): number | null {
      const taken = status?.error?.match(/^Port (\d+) is used by another program/);
      return taken ? Number(taken[1]) + 1 : null;
    },
    url(site: Site) {
      return siteUrl(site, ...publicPorts(list));
    },
    /** macOS: ports 80 and 443 reach the sites. */
    get forwarded() {
      return !!list.forwarded;
    },
    /** macOS: offer ports 80 and 443 (after .test names work, which the sites need first). */
    get offerForward() {
      return !!list.canForward && !list.forwarded && !!api.webForward && this.setup !== 'dns';
    },
    forward: (on: boolean) => act(null, () => api.webForward!(on)),
    /** The one setup step to offer: .test names first, then trust once a site uses HTTPS. */
    get setup(): 'dns' | 'trust' | null {
      if (!status) return null;
      if (!status.dns.ready && status.dns.problem) return 'dns';
      if (!status.caTrusted && list.sites.some((s) => s.secured)) return 'trust';
      return null;
    },
    setupDns: () => act(null, () => api.dnsSetup()),
    trust: () => act(null, () => api.tlsTrust()),
    async secure(name: string, on: boolean) {
      const site = list.sites.find((s) => s.name === name);
      if (!site) return;
      const before = site.secured;
      site.secured = on;
      errors[name] = null;
      try {
        await (on ? api.sitesSecure(name) : api.sitesUnsecure(name));
        await reload();
      } catch (e) {
        site.secured = before;
        errors[name] = asMessage(e);
      }
    },
    note(name: string) {
      return status?.notes[name] ?? null;
    },
    load: reload,
    async refreshStatus() {
      [status] = await Promise.all([api.webStatus(), checkIde()]);
    },
    /** Whether an IDE listens for Xdebug, in a sentence. */
    get ide() {
      if (!ide) return '';
      if (!ide.listening) return `No IDE is listening on port ${ide.port}. Start listening for Xdebug in your IDE.`;
      return ide.program
        ? `${ide.program} is listening for Xdebug on port ${ide.port}`
        : `A program is listening for Xdebug on port ${ide.port}`;
    },
    get ideListening() {
      return !!ide?.listening;
    },
    /** Starts (or stops) an Xdebug session for every request of the site; rolls back when refused. */
    debug: (name: string, on: boolean) => toggle(name, 'debug', on, () => api.sitesDebug(name, on)),
    /** Lets SPX profile (or stop profiling) every request of the site; rolls back when refused. */
    profile: (name: string, on: boolean) => toggle(name, 'profile', on, () => api.sitesProfile(name, on)),
    /** Whether the PHP the site runs on has SPX (a removed isolated version falls back to the default). */
    canProfile(site: Site) {
      const has = (minor: string | null) => php.installed.find((i) => i.minor === minor);
      const runs = has(site.php) ?? has(php.default);
      // A profiled site keeps its switch: it must stay possible to switch it off.
      return site.kind !== 'proxy' && (site.profile || (!!runs && 'spx' in runs.extensions));
    },
    park: (dir: string) => act(null, () => api.sitesPark(dir)),
    unpark: (dir: string) => act(null, () => api.sitesUnpark(dir)),
    link: (name: string, dir: string) => act(null, () => api.sitesLink(name, dir)),
    unlink: (name: string) => act(name, () => api.sitesUnlink(name)),
    proxy: (name: string, upstream: string) => act(null, () => api.sitesProxy(name, upstream)),
    unproxy: (name: string) => act(name, () => api.sitesUnproxy(name)),
    setPort: (port: number) => act(null, () => api.sitesPort(port)),
    async isolate(name: string, minor: string | null) {
      const site = list.sites.find((s) => s.name === name);
      if (!site) return;
      const before = site.php;
      site.php = minor;
      errors[name] = null;
      try {
        await api.sitesIsolate(name, minor);
      } catch (e) {
        site.php = before;
        errors[name] = asMessage(e);
      }
    },
  };
}

export type SitesStore = ReturnType<typeof createSitesStore>;
