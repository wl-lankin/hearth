/** Hearth's pages and how the menu groups them. */

export type View =
  | 'home'
  | 'sites'
  | 'workspaces'
  | 'services'
  | 'processes'
  | 'runtimes'
  | 'mail'
  | 'inspect'
  | 'logs'
  | 'settings'
  | 'moveover';

export type NavItem = { id: View; label: string };

/** The rail, top to bottom. Settings is pinned to the bottom of the rail and not listed here;
 *  the move over from Herd opens from Home and Settings only. */
export const groups: { label: string | null; items: NavItem[] }[] = [
  { label: null, items: [{ id: 'home', label: 'Home' }] },
  {
    label: 'Serve',
    items: [
      { id: 'sites', label: 'Sites' },
      { id: 'workspaces', label: 'Workspaces' },
    ],
  },
  {
    label: 'Run',
    items: [
      { id: 'services', label: 'Services' },
      { id: 'processes', label: 'Processes' },
      { id: 'runtimes', label: 'Runtimes' },
    ],
  },
  {
    label: 'Debug',
    items: [
      { id: 'mail', label: 'Mail' },
      { id: 'inspect', label: 'Inspect' },
      { id: 'logs', label: 'Logs' },
    ],
  },
];

const views = new Set<string>([...groups.flatMap((g) => g.items.map((i) => i.id)), 'settings', 'moveover']);

/** Pages that became a tab of a merged page; old `?view=` links still open them. */
const merged: Record<string, { view: View; tab: string }> = {
  php: { view: 'runtimes', tab: 'php' },
  node: { view: 'runtimes', tab: 'node' },
  dumps: { view: 'inspect', tab: 'dumps' },
  requests: { view: 'inspect', tab: 'requests' },
  profiles: { view: 'inspect', tab: 'profiles' },
};

/** The page (and tab) a `?view=` value opens; anything unknown opens Home. */
export function resolveView(param: string | null): { view: View; tab: string | null } {
  if (param && param in merged) return merged[param];
  if (param && views.has(param)) return { view: param as View, tab: null };
  return { view: 'home', tab: null };
}

export function unreadCount(mails: { seen: boolean }[]): number {
  return mails.filter((m) => !m.seen).length;
}
