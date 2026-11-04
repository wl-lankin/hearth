import type { ProfileReport, ProfileStatus, SitesList } from './api';

export type ProfilesApi = {
  profileStatus(): Promise<ProfileStatus>;
  profileList(): Promise<{ reports: ProfileReport[] }>;
  profileClear(): Promise<{ deleted: number }>;
  sitesList(): Promise<SitesList>;
  openUrl(url: string): Promise<void>;
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createProfilesStore(api: ProfilesApi) {
  let status = $state<ProfileStatus | null>(null);
  let reports = $state<ProfileReport[]>([]);
  let sites = $state<SitesList>({ sites: [], parked: [], httpPort: 8080, httpsPort: 8443 });
  let error = $state<string | null>(null);

  async function load() {
    error = null;
    try {
      status = await api.profileStatus();
      if (!status.available) {
        reports = [];
        return;
      }
      [{ reports }, sites] = await Promise.all([api.profileList(), api.sitesList()]);
    } catch (e) {
      error = asMessage(e);
    }
  }

  /**
   * SPX's own report page for `r`, served by PHP through a site: the report's site, or for a
   * command the first PHP site; null when there is none.
   */
  function reportUrl(r: ProfileReport): string | null {
    if (!status) return null;
    const php = sites.sites.filter((s) => s.kind !== 'proxy');
    const site = php.find((s) => `${s.name}.test` === r.host) ?? (r.cli || !r.host ? php[0] : undefined);
    if (!site) return null;
    const base = site.secured ? `https://${site.name}.test:${sites.httpsPort}` : `http://${site.name}.test:${sites.httpPort}`;
    return `${base}/?SPX_KEY=${encodeURIComponent(status.key)}&SPX_UI_URI=/report.html&key=${encodeURIComponent(r.key)}`;
  }

  return {
    get reports() {
      return reports;
    },
    get available() {
      return status?.available ?? false;
    },
    /** Why there is nothing to list, when no PHP has SPX. */
    get unavailable() {
      return status && !status.available ? "Profiling needs SPX, which Hearth's macOS PHP builds include. Windows builds have no SPX." : null;
    },
    get error() {
      return error;
    },
    reportUrl,
    load,
    async open(r: ProfileReport) {
      const url = reportUrl(r);
      if (url) await api.openUrl(url);
    },
    async clear() {
      error = null;
      try {
        await api.profileClear();
        await load();
      } catch (e) {
        error = asMessage(e);
      }
    },
  };
}
