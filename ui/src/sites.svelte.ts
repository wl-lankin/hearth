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
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createSitesStore(api: SitesApi) {
  let list = $state<SitesList>({ sites: [], parked: [], httpPort: 8080 });
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
    /** The next port when the current one is taken by another program. */
    get portSuggestion(): number | null {
      const taken = status?.error?.match(/^Port (\d+) is used by another program/);
      return taken ? Number(taken[1]) + 1 : null;
    },
    url(site: Site) {
      return `http://${site.name}.test:${list.httpPort}`;
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
