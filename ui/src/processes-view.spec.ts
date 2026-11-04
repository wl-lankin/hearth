// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test } from 'vitest';
import Processes from './views/Processes.svelte';

test('the Processes page renders the demo processes and their output without looping', async () => {
  const target = document.createElement('div');
  document.body.append(target);
  const page = mount(Processes, { target });
  for (let i = 0; i < 5; i++) {
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
  }
  expect(target.textContent).toContain('shop.test');
  expect(target.textContent).toContain('VITE v7.1.4');
  unmount(page);
});
