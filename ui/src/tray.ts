/** Helpers of the tray panel. */

/** A site's address, as Stoke serves it: HTTPS when secured, the port only when it is not the default. */
export function siteUrl(site: { name: string; secured: boolean }, httpPort: number, httpsPort: number): string {
  const [scheme, port, standard] = site.secured ? ['https', httpsPort, 443] : ['http', httpPort, 80];
  return `${scheme}://${site.name}.test${port === standard ? '' : `:${port}`}`;
}

/** The sites the panel lists (the first `max` by name) and how many there are in all when that is more. */
export function shownSites<T extends { name: string }>(sites: T[], max = 5): { shown: T[]; more: number } {
  const sorted = [...sites].sort((a, b) => a.name.localeCompare(b.name));
  return { shown: sorted.slice(0, max), more: sites.length > max ? sites.length : 0 };
}
