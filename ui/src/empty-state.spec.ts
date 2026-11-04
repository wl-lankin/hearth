// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test, vi } from 'vitest';
import EmptyState from './EmptyState.svelte';
import Sites from './views/Sites.svelte';

test('an empty page shows its illustration, title, line and button', () => {
  const target = document.createElement('div');
  document.body.append(target);
  const onclick = vi.fn();
  const page = mount(EmptyState, {
    target,
    props: { art: '/sites.webp', title: 'No sites yet', line: 'Link a folder', action: { label: 'Link a folder', onclick } },
  });
  flushSync();
  const img = target.querySelector('img')!;
  expect(img.getAttribute('src')).toBe('/sites.webp');
  expect(img.getAttribute('alt')).toBe('');
  expect(target.textContent).toContain('No sites yet');
  target.querySelector('button')!.click();
  expect(onclick).toHaveBeenCalledTimes(1);
  unmount(page);

  const quiet = mount(EmptyState, { target, props: { art: '/logs.webp', title: 'All quiet', line: 'No errors' } });
  flushSync();
  expect(target.querySelector('button')).toBeNull();
  unmount(quiet);
});

test('Sites can open with the link form', async () => {
  const target = document.createElement('div');
  document.body.append(target);
  const page = mount(Sites, { target, props: { startLinking: true } });
  for (let i = 0; i < 5; i++) {
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
  }
  expect([...target.querySelectorAll('button')].some((b) => b.textContent?.trim() === 'Choose')).toBe(true);
  unmount(page);
});
