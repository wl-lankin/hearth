import type { PhpList, Site, SitesList, WebStatus } from './api';

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
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createSitesStore(api: SitesApi) {
  let list = $state<SitesList>({ sites: [], parked: [], httpPort: 8080, httpsPort: 8443 });
  let status = $state<WebStatus | null>(null);
  let php = $state<PhpList>({ default: null, installed: [] });
  let errors = $state<Record<string, string | null>>({});
  let error = $state<string | null>(null);

  async function reload() {
    [list, status, php] = await Promise.all([api.sitesList(), api.webStatus(), api.phpList()]);
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
    /** The one-line web server state above the sites. */
    get server(): { kind: 'error' | 'running' | 'stopped' | 'idle'; text: string } {
      if (status?.error) return { kind: 'error', text: status.error };
      if (status?.running) return { kind: 'running', text: `Serving on port ${list.httpPort}` };
      if (list.sites.length > 0) return { kind: 'stopped', text: 'Not running yet' };
      return { kind: 'idle', text: 'Not serving yet: park or link a folder' };
    },
    /** The next port when the current one is taken by another program. */
    get portSuggestion(): number | null {
      const taken = status?.error?.match(/^Port (\d+) is used by another program/);
      return taken ? Number(taken[1]) + 1 : null;
    },
    url(site: Site) {
      return site.secured ? `https://${site.name}.test:${list.httpsPort}` : `http://${site.name}.test:${list.httpPort}`;
    },
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
      status = await api.webStatus();
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
