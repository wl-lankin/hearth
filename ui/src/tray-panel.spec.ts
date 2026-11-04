// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test, vi } from 'vitest';
import * as api from './api';
import TrayPanel from './TrayPanel.svelte';

async function settle() {
  for (let i = 0; i < 6; i++) {
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
  }
}

function fakes(overrides: Partial<typeof api> = {}) {
  const source = {
    ...api,
    phpList: async () => ({
      default: '8.4',
      installed: [
        { minor: '8.4', version: '8.4.27', extensions: {} },
        { minor: '8.3', version: '8.3.19', extensions: {} },
      ],
    }),
    phpUse: vi.fn(async () => {}),
    servicesStart: vi.fn(async () => {}),
    servicesStop: vi.fn(async () => {}),
    openUrl: vi.fn(async () => {}),
    installUpdateNow: vi.fn(async () => {}),
    ...overrides,
  } as typeof api;
  const actions = { open: vi.fn(), quit: vi.fn(), fit: vi.fn(), hide: vi.fn() };
  return { source, actions };
}

async function render(overrides: Partial<typeof api> = {}) {
  const { source, actions } = fakes(overrides);
  const target = document.createElement('div');
  document.body.append(target);
  const panel = mount(TrayPanel, { target, props: { source, actions } });
  await settle();
  const text = () => target.textContent?.replace(/\s+/g, ' ') ?? '';
  const button = (label: string) =>
    [...target.querySelectorAll('button')].find((b) => b.textContent?.replace(/\s+/g, ' ').trim().startsWith(label))!;
  return { target, panel, source, actions, text, button };
}

test('the panel shows status, PHP, services and sites', async () => {
  const { target, panel, text } = await render();
  expect(text()).toContain('Serving 4 sites');
  expect(text()).toContain('Default version');
  expect(text()).toContain('8.4');
  const switches = [...target.querySelectorAll('[role="switch"]')].map((s) => s.getAttribute('aria-checked'));
  expect(switches).toEqual(['true', 'true', 'false', 'false']);
  expect(text()).toContain('shop.test');
  expect(text()).not.toContain('All 4 sites');
  unmount(panel);
});

test('the PHP flyout switches the default version and offers more', async () => {
  const { panel, source, actions, text, button } = await render();
  button('Default version').click();
  flushSync();
  expect(text()).toContain('8.3.19');
  expect(text()).toContain('Install another...');
  button('PHP 8.3').click();
  await settle();
  expect(source.phpUse).toHaveBeenCalledWith('8.3');
  expect(text()).not.toContain('Install another...');
  button('Default version').click();
  flushSync();
  button('Install another...').click();
  expect(actions.open).toHaveBeenCalledWith('runtimes', 'php');
  unmount(panel);
});

test('switches start and stop services, sites open in the browser', async () => {
  const { target, panel, source } = await render();
  const switches = [...target.querySelectorAll<HTMLButtonElement>('[role="switch"]')];
  switches[0].click();
  switches[2].click();
  await settle();
  expect(source.servicesStop).toHaveBeenCalledWith('mysql');
  expect(source.servicesStart).toHaveBeenCalledWith('postgresql');
  [...target.querySelectorAll('button')].find((b) => b.querySelector('.name')?.textContent === 'shop.test')!.click();
  expect(source.openUrl).toHaveBeenCalledWith('https://shop.test:8443');
  unmount(panel);
});

test('many sites: five rows and a link to all of them', async () => {
  const many = Array.from({ length: 40 }, (_, i) => ({
    name: `site${i}`, kind: 'parked' as const, path: '', upstream: null, docroot: null, php: null,
    secured: false, debug: false, profile: false, problem: null,
  }));
  const { panel, actions, text, button } = await render({
    sitesList: async () => ({ httpPort: 80, httpsPort: 443, parked: [], sites: many }),
  });
  expect(text()).toContain('All 40 sites...');
  button('All 40 sites...').click();
  expect(actions.open).toHaveBeenCalledWith('sites');
  unmount(panel);
});

test('a downloaded update offers the restart; Open, Quit and Esc reach the app', async () => {
  const { panel, source, actions, text, button } = await render({
    updateState: async () => ({ phase: 'ready', version: '0.2.0' }) as Awaited<ReturnType<typeof api.updateState>>,
  });
  expect(text()).toContain('Hearth 0.2.0 is ready');
  button('Restart to update').click();
  expect(source.installUpdateNow).toHaveBeenCalled();
  button('Open Hearth').click();
  expect(actions.open).toHaveBeenCalledWith();
  button('Quit Hearth').click();
  expect(actions.quit).toHaveBeenCalled();
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
  expect(actions.hide).toHaveBeenCalled();
  unmount(panel);
});

test('without a core the panel says so and nothing breaks', async () => {
  const down = () => Promise.reject(new Error('Hearth core is not running'));
  const { panel, text } = await render({
    phpList: down, servicesList: down, sitesList: down, webStatus: down, updateState: down,
  });
  expect(text()).toContain('Hearth core is not running');
  expect(text()).not.toContain('Restart to update');
  unmount(panel);
});

test('without PHP the row says None and the flyout still offers an install', async () => {
  const { panel, actions, text, button } = await render({ phpList: async () => ({ default: null, installed: [] }) });
  expect(text()).toContain('None');
  button('Default version').click();
  flushSync();
  button('Install another...').click();
  expect(actions.open).toHaveBeenCalledWith('runtimes', 'php');
  unmount(panel);
});
