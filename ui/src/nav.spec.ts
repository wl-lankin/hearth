import { expect, test } from 'vitest';
import { groups, resolveView, unreadCount } from './nav';

test('old links open the merged pages on the right tab', () => {
  expect(resolveView('php')).toEqual({ view: 'runtimes', tab: 'php' });
  expect(resolveView('node')).toEqual({ view: 'runtimes', tab: 'node' });
  for (const tab of ['dumps', 'requests', 'profiles']) expect(resolveView(tab)).toEqual({ view: 'inspect', tab });
  expect(resolveView('dashboard')).toEqual({ view: 'home', tab: null });
  expect(resolveView('sites')).toEqual({ view: 'sites', tab: null });
  expect(resolveView('nope')).toEqual({ view: 'home', tab: null });
  expect(resolveView(null)).toEqual({ view: 'home', tab: null });
});

test('unread mail is counted', () => {
  expect(unreadCount([{ seen: true }, { seen: false }, { seen: false }])).toBe(2);
  expect(unreadCount([])).toBe(0);
});

test('the menu is grouped', () => {
  expect(groups.flatMap((g) => g.items.map((i) => i.label))).toEqual([
    'Home', 'Sites', 'Workspaces', 'Services', 'Processes', 'Runtimes', 'Mail', 'Inspect', 'Logs',
  ]);
  expect(groups.map((g) => g.label)).toEqual([null, 'Serve', 'Run', 'Debug']);
});
