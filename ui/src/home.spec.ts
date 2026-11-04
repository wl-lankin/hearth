// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test, vi } from 'vitest';
import * as api from './api';
import Home from './views/Home.svelte';

async function settle() {
  for (let i = 0; i < 6; i++) {
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
  }
}

function render(source: typeof api, onnavigate = vi.fn()) {
  const target = document.createElement('div');
  document.body.append(target);
  const page = mount(Home, { target, props: { onnavigate, source } });
  return { target, page, onnavigate };
}

test('Home greets and shows sites, running services and the default PHP', async () => {
  const { target, page } = render(api);
  await settle();
  const text = target.textContent ?? '';
  expect(text).toMatch(/Good (morning|afternoon|evening|night)/);
  expect(text).toContain('4 sites, 2 services running');
  const tiles = [...target.querySelectorAll('.stat')].map((t) => t.textContent?.replace(/\s+/g, ' ').trim());
  expect(tiles).toEqual(['4 Sites', '2 Services running', '8.4 PHP']);
  unmount(page);
});

test('without an answer from the core there is no welcome, and nothing breaks', async () => {
  const failing = { ...api, sitesList: () => Promise.reject(new Error('Hearth core is not running')) };
  const { target, page } = render(failing);
  await settle();
  expect(target.textContent).not.toContain('Welcome to Hearth');
  expect(target.textContent).toContain('Core');
  unmount(page);
});

test('with no sites yet Home welcomes and Get started opens the link form', async () => {
  const empty = { ...api, sitesList: async () => ({ ...(await api.sitesList()), sites: [] }) };
  const { target, page, onnavigate } = render(empty);
  await settle();
  expect(target.textContent).toContain('Welcome to Hearth');
  [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Get started')!.click();
  expect(onnavigate).toHaveBeenCalledWith('sites', { link: true });
  unmount(page);
});
