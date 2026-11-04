// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test } from 'vitest';
import App from './App.svelte';

async function settle() {
  for (let i = 0; i < 5; i++) {
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
  }
}

test('an old link to Profiles opens Inspect on its Profiles tab', async () => {
  history.replaceState(null, '', '/?view=profiles');
  const target = document.createElement('div');
  document.body.append(target);
  const app = mount(App, { target });
  await settle();
  const rail = target.querySelector('nav')!;
  expect(rail.querySelector('button[aria-current="page"]')?.textContent?.trim()).toBe('Inspect');
  expect(target.querySelector('[role="tab"][aria-selected="true"]')?.textContent?.trim()).toBe('Profiles');
  const labels = [...target.querySelectorAll('aside button')].map((b) => b.textContent?.trim().replace(/\d+$/, ''));
  expect(labels).toHaveLength(10);
  expect(labels).toContain('Settings');
  unmount(app);
});
